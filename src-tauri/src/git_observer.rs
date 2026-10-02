use std::collections::HashSet;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant};

const GIT_TIMEOUT: Duration = Duration::from_secs(12);

#[derive(Debug, Clone)]
pub(crate) struct GitRepository {
    pub(crate) path: PathBuf,
    pub(crate) branch: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GitArtifact {
    pub(crate) path: String,
    pub(crate) commit: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ArtifactLookup {
    Found(GitArtifact),
    None,
    Ambiguous,
}

pub(crate) fn fetch_and_head(repository: &GitRepository) -> Result<String, String> {
    run_git(&repository.path, ["fetch", "origin", &repository.branch])?;
    remote_head(repository)
}

pub(crate) fn remote_head(repository: &GitRepository) -> Result<String, String> {
    run_git(
        &repository.path,
        [
            "rev-parse",
            "--verify",
            &format!("origin/{}", repository.branch),
        ],
    )
}

#[allow(dead_code)]
pub(crate) fn fetch_head_with_timestamp(
    repository: &GitRepository,
) -> Result<(String, u64), String> {
    let head = fetch_and_head(repository)?;
    Ok((head.clone(), commit_timestamp(repository, &head)?))
}

pub(crate) fn is_ancestor_of(
    repository: &GitRepository,
    ancestor: &str,
    descendant: &str,
) -> Result<bool, String> {
    let output = run_git_allow_status(
        &repository.path,
        ["merge-base", "--is-ancestor", ancestor, descendant],
    )?;
    match output.status.code() {
        Some(0) => Ok(true),
        Some(1) => Ok(false),
        _ => Err(format!(
            "git merge-base --is-ancestor failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )),
    }
}

pub(crate) fn newest_added_artifact_between(
    repository: &GitRepository,
    baseline: &str,
    head: &str,
    directory: &str,
    accepted_suffixes: &[&str],
) -> Result<ArtifactLookup, String> {
    select_created_artifact(
        repository,
        &format!("{baseline}..{head}"),
        head,
        directory,
        accepted_suffixes,
    )
}

pub(crate) fn newest_current_artifact_at(
    repository: &GitRepository,
    head: &str,
    directory: &str,
    accepted_suffixes: &[&str],
) -> Result<ArtifactLookup, String> {
    select_created_artifact(repository, head, head, directory, accepted_suffixes)
}

fn select_created_artifact(
    repository: &GitRepository,
    revision: &str,
    tree_head: &str,
    directory: &str,
    accepted_suffixes: &[&str],
) -> Result<ArtifactLookup, String> {
    let existing = run_git(
        &repository.path,
        ["ls-tree", "-r", "--name-only", tree_head, "--", directory],
    )?
    .lines()
    .map(normalize_path)
    .collect::<HashSet<_>>();
    let output = run_git(
        &repository.path,
        [
            "log",
            "--topo-order",
            "--name-status",
            "--format=commit:%H",
            revision,
            "--",
            directory,
        ],
    )?;
    let mut commit = None::<String>;
    let mut candidates = Vec::<String>::new();
    for line in output.lines().chain(std::iter::once("commit:")) {
        if let Some(next) = line.strip_prefix("commit:") {
            if let Some(commit) = commit.take() {
                match select_in_commit(&candidates) {
                    CommitSelection::Found(path) => {
                        return Ok(ArtifactLookup::Found(GitArtifact { path, commit }))
                    }
                    CommitSelection::Ambiguous => return Ok(ArtifactLookup::Ambiguous),
                    CommitSelection::None => {}
                }
            }
            candidates.clear();
            if !next.is_empty() {
                commit = Some(next.to_string());
            }
            continue;
        }
        let Some((status, path)) = line.split_once('\t') else {
            continue;
        };
        let path = normalize_path(path);
        if status == "A"
            && path.starts_with(directory)
            && existing.contains(&path)
            && accepted_suffixes
                .iter()
                .any(|suffix| path.ends_with(suffix))
        {
            candidates.push(path);
        }
    }
    Ok(ArtifactLookup::None)
}

enum CommitSelection {
    Found(String),
    None,
    Ambiguous,
}

fn select_in_commit(candidates: &[String]) -> CommitSelection {
    if candidates.is_empty() {
        return CommitSelection::None;
    }
    if candidates.len() == 1 {
        return CommitSelection::Found(candidates[0].clone());
    }
    let mut ordered = candidates
        .iter()
        .map(|path| (artifact_filename_timestamp(path), path))
        .collect::<Vec<_>>();
    if ordered.iter().any(|(timestamp, _)| timestamp.is_none()) {
        return CommitSelection::Ambiguous;
    }
    ordered.sort_by(|left, right| left.0.cmp(&right.0));
    let (timestamp, path) = ordered.last().expect("candidates is non-empty");
    if ordered
        .iter()
        .filter(|(other, _)| other == timestamp)
        .count()
        != 1
    {
        return CommitSelection::Ambiguous;
    }
    CommitSelection::Found((*path).clone())
}

fn artifact_filename_timestamp(path: &str) -> Option<&str> {
    let file = Path::new(path).file_name()?.to_str()?;
    let timestamp = file.get(..15)?;
    let (date, time) = timestamp.split_once('-')?;
    (date.len() == 8
        && time.len() == 6
        && date.bytes().all(|b| b.is_ascii_digit())
        && time.bytes().all(|b| b.is_ascii_digit()))
    .then_some(timestamp)
}

fn normalize_path(path: &str) -> String {
    path.replace('\\', "/")
}

pub(crate) fn commit_timestamp(repository: &GitRepository, commit: &str) -> Result<u64, String> {
    run_git(&repository.path, ["show", "-s", "--format=%ct", commit])?
        .parse::<u64>()
        .map_err(|error| format!("invalid Git timestamp: {error}"))
}

fn run_git<I, S>(directory: &Path, arguments: I) -> Result<String, String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let args = collect_args(arguments);
    let output = run_git_allow_status(directory, &args)?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        Err(format!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }
}

fn run_git_allow_status<I, S>(directory: &Path, arguments: I) -> Result<GitOutput, String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    if !directory.is_dir() {
        return Err(format!(
            "Git checkout is unavailable: {}",
            directory.display()
        ));
    }
    let args = collect_args(arguments);
    let mut command = Command::new("git");
    command
        .args(&args)
        .current_dir(directory)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GCM_INTERACTIVE", "Never");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    wait_for_git(
        command
            .spawn()
            .map_err(|error| format!("Unable to start git: {error}"))?,
        &format!("git {}", args.join(" ")),
    )
}

fn collect_args<I, S>(arguments: I) -> Vec<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    arguments
        .into_iter()
        .map(|argument| argument.as_ref().to_string())
        .collect()
}

struct GitOutput {
    status: ExitStatus,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

fn wait_for_git(mut child: std::process::Child, description: &str) -> Result<GitOutput, String> {
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "git stdout was unavailable".to_string())?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| "git stderr was unavailable".to_string())?;
    // Drain both pipes before polling so verbose Git never blocks behind a full pipe.
    let stdout_reader = thread::spawn(move || read_all(stdout));
    let stderr_reader = thread::spawn(move || read_all(stderr));
    let started = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().map_err(|error| error.to_string())? {
            break status;
        }
        if started.elapsed() >= GIT_TIMEOUT {
            let _ = child.kill();
            let _ = child.wait();
            let _ = stdout_reader.join();
            let _ = stderr_reader.join();
            return Err(format!("{description} timed out"));
        }
        thread::sleep(Duration::from_millis(30));
    };
    let stdout = stdout_reader
        .join()
        .map_err(|_| "git stdout reader panicked".to_string())??;
    let stderr = stderr_reader
        .join()
        .map_err(|_| "git stderr reader panicked".to_string())??;
    Ok(GitOutput {
        status,
        stdout,
        stderr,
    })
}

fn read_all<R: Read>(mut reader: R) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    reader
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::atomic::{AtomicUsize, Ordering};
    static TEMP_SEQUENCE: AtomicUsize = AtomicUsize::new(0);
    fn temp_repository() -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "iseol-git-observer-{}-{}",
            std::process::id(),
            TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).unwrap();
        run_git(&path, ["init"]).unwrap();
        run_git(&path, ["config", "user.email", "test@example.invalid"]).unwrap();
        run_git(&path, ["config", "user.name", "test"]).unwrap();
        path
    }
    fn commit(path: &Path, message: &str) {
        run_git(path, ["add", "."]).unwrap();
        run_git(path, ["commit", "-m", message]).unwrap();
    }
    #[test]
    fn unavailable_checkout_returns_a_bounded_error() {
        assert!(fetch_and_head(&GitRepository {
            path: PathBuf::from("Z:/missing-workflow-repository"),
            branch: "main".to_string()
        })
        .is_err());
    }
    #[test]
    fn selects_current_tree_addition_by_same_commit_filename_timestamp() {
        let path = temp_repository();
        fs::create_dir_all(path.join("noctua/prepare")).unwrap();
        fs::write(
            path.join("noctua/prepare/20261002-150000-a-execution-handoff.md"),
            "one",
        )
        .unwrap();
        fs::write(
            path.join("noctua/prepare/20261002-150100-b-qa-handoff.md"),
            "two",
        )
        .unwrap();
        commit(&path, "add handoffs");
        let head = run_git(&path, ["rev-parse", "HEAD"]).unwrap();
        let repository = GitRepository {
            path: path.clone(),
            branch: "main".to_string(),
        };
        assert_eq!(
            newest_current_artifact_at(
                &repository,
                &head,
                "noctua/prepare/",
                &["-execution-handoff.md", "-qa-handoff.md"]
            )
            .unwrap(),
            ArtifactLookup::Found(GitArtifact {
                path: "noctua/prepare/20261002-150100-b-qa-handoff.md".to_string(),
                commit: head.clone()
            })
        );
        fs::remove_file(path.join("noctua/prepare/20261002-150100-b-qa-handoff.md")).unwrap();
        commit(&path, "delete qa");
        let deleted_head = run_git(&path, ["rev-parse", "HEAD"]).unwrap();
        assert_eq!(
            newest_current_artifact_at(
                &repository,
                &deleted_head,
                "noctua/prepare/",
                &["-qa-handoff.md"]
            )
            .unwrap(),
            ArtifactLookup::None
        );
        let _ = fs::remove_dir_all(path);
    }
    #[test]
    fn modification_is_not_a_new_completion_in_a_range() {
        let path = temp_repository();
        fs::create_dir_all(path.join("noctua/prepare")).unwrap();
        fs::write(
            path.join("noctua/prepare/20261002-150000-a-qa-handoff.md"),
            "one",
        )
        .unwrap();
        commit(&path, "add");
        let baseline = run_git(&path, ["rev-parse", "HEAD"]).unwrap();
        fs::write(
            path.join("noctua/prepare/20261002-150000-a-qa-handoff.md"),
            "changed",
        )
        .unwrap();
        commit(&path, "modify");
        let head = run_git(&path, ["rev-parse", "HEAD"]).unwrap();
        let repository = GitRepository {
            path: path.clone(),
            branch: "main".to_string(),
        };
        assert_eq!(
            newest_added_artifact_between(
                &repository,
                &baseline,
                &head,
                "noctua/prepare/",
                &["-qa-handoff.md"]
            )
            .unwrap(),
            ArtifactLookup::None
        );
        let _ = fs::remove_dir_all(path);
    }

    #[test]
    fn pinned_head_remains_stable_after_the_checkout_moves_forward() {
        let path = temp_repository();
        fs::create_dir_all(path.join("noctua/prepare")).unwrap();
        fs::write(
            path.join("noctua/prepare/20261002-150000-a-execution-handoff.md"),
            "one",
        )
        .unwrap();
        commit(&path, "s1");
        let s1 = run_git(&path, ["rev-parse", "HEAD"]).unwrap();
        fs::write(
            path.join("noctua/prepare/20261002-150100-b-qa-handoff.md"),
            "two",
        )
        .unwrap();
        commit(&path, "s2");
        let s2 = run_git(&path, ["rev-parse", "HEAD"]).unwrap();
        let repository = GitRepository {
            path: path.clone(),
            branch: "main".to_string(),
        };
        assert_eq!(
            newest_current_artifact_at(
                &repository,
                &s1,
                "noctua/prepare/",
                &["-execution-handoff.md", "-qa-handoff.md"],
            )
            .unwrap(),
            ArtifactLookup::Found(GitArtifact {
                path: "noctua/prepare/20261002-150000-a-execution-handoff.md".to_string(),
                commit: s1.clone()
            }),
        );
        assert_eq!(
            newest_added_artifact_between(
                &repository,
                &s1,
                &s2,
                "noctua/prepare/",
                &["-qa-handoff.md"]
            )
            .unwrap(),
            ArtifactLookup::Found(GitArtifact {
                path: "noctua/prepare/20261002-150100-b-qa-handoff.md".to_string(),
                commit: s2
            }),
        );
        let _ = fs::remove_dir_all(path);
    }
    #[test]
    fn drains_more_than_a_pipe_buffer_while_git_runs() {
        let path = temp_repository();
        for index in 0..4_000 {
            let file = path.join(format!("large/{index:05}-{}", "x".repeat(40)));
            fs::create_dir_all(file.parent().unwrap()).unwrap();
            fs::write(file, "x").unwrap();
        }
        commit(&path, "large tree");
        let output = run_git(&path, ["ls-tree", "-r", "HEAD"]).unwrap();
        assert!(
            output.len() > 128 * 1024,
            "output was only {} bytes",
            output.len()
        );
        let _ = fs::remove_dir_all(path);
    }

    #[test]
    fn same_commit_without_filename_timestamp_is_ambiguous() {
        assert!(matches!(
            select_in_commit(&[
                "noctua/prepare/first-qa-handoff.md".to_string(),
                "noctua/prepare/second-qa-handoff.md".to_string(),
            ]),
            CommitSelection::Ambiguous
        ));
    }
}
