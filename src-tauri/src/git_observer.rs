use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

const GIT_TIMEOUT: Duration = Duration::from_secs(12);

#[derive(Debug, Clone)]
pub(crate) struct GitRepository {
    pub(crate) path: PathBuf,
    pub(crate) branch: String,
}

#[derive(Debug, Clone)]
pub(crate) struct GitArtifact {
    pub(crate) path: String,
    pub(crate) commit: String,
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

pub(crate) fn fetch_head_with_timestamp(
    repository: &GitRepository,
) -> Result<(String, u64), String> {
    let head = fetch_and_head(repository)?;
    let timestamp = commit_timestamp(repository, &head)?;
    Ok((head, timestamp))
}

pub(crate) fn is_ancestor(repository: &GitRepository, ancestor: &str) -> Result<bool, String> {
    let output = run_git_allow_status(
        &repository.path,
        [
            "merge-base",
            "--is-ancestor",
            ancestor,
            &format!("origin/{}", repository.branch),
        ],
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

pub(crate) fn newest_added_artifact(
    repository: &GitRepository,
    baseline: &str,
    directory: &str,
    accepted_suffixes: &[&str],
) -> Result<Option<GitArtifact>, String> {
    let range = format!("{baseline}..origin/{}", repository.branch);
    let output = run_git(
        &repository.path,
        [
            "log",
            "--name-status",
            "--format=commit:%H",
            &range,
            "--",
            directory,
        ],
    )?;
    let mut current_commit = None;
    for line in output.lines() {
        if let Some(commit) = line.strip_prefix("commit:") {
            current_commit = Some(commit.to_string());
            continue;
        }
        let Some((status, path)) = line.split_once('\t') else {
            continue;
        };
        if status == "A"
            && accepted_suffixes
                .iter()
                .any(|suffix| path.ends_with(suffix))
            && path.replace('\\', "/").starts_with(directory)
        {
            return Ok(current_commit.map(|commit| GitArtifact {
                path: path.replace('\\', "/"),
                commit,
            }));
        }
    }
    Ok(None)
}

pub(crate) fn newest_current_artifact(
    repository: &GitRepository,
    directory: &str,
    accepted_suffixes: &[&str],
) -> Result<Option<GitArtifact>, String> {
    let output = run_git(
        &repository.path,
        [
            "log",
            "--name-status",
            "--format=commit:%H",
            &format!("origin/{}", repository.branch),
            "--",
            directory,
        ],
    )?;
    let mut current_commit = None;
    for line in output.lines() {
        if let Some(commit) = line.strip_prefix("commit:") {
            current_commit = Some(commit.to_string());
            continue;
        }
        let Some((_, path)) = line.split_once('\t') else {
            continue;
        };
        if accepted_suffixes
            .iter()
            .any(|suffix| path.ends_with(suffix))
            && path.replace('\\', "/").starts_with(directory)
        {
            return Ok(current_commit.map(|commit| GitArtifact {
                path: path.replace('\\', "/"),
                commit,
            }));
        }
    }
    Ok(None)
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
    if !directory.is_dir() {
        return Err(format!(
            "Git checkout is unavailable: {}",
            directory.display()
        ));
    }
    let args = arguments
        .into_iter()
        .map(|argument| argument.as_ref().to_string())
        .collect::<Vec<_>>();
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
        command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let output = wait_for_git(
        command
            .spawn()
            .map_err(|error| format!("Unable to start git: {error}"))?,
        &format!("git {}", args.join(" ")),
    )?;
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

fn run_git_allow_status<I, S>(
    directory: &Path,
    arguments: I,
) -> Result<std::process::Output, String>
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
    let args = arguments
        .into_iter()
        .map(|argument| argument.as_ref().to_string())
        .collect::<Vec<_>>();
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
    let child = command
        .spawn()
        .map_err(|error| format!("Unable to start git: {error}"))?;
    wait_for_git(child, &format!("git {}", args.join(" ")))
}

fn wait_for_git(
    mut child: std::process::Child,
    description: &str,
) -> Result<std::process::Output, String> {
    let started = Instant::now();
    loop {
        if child
            .try_wait()
            .map_err(|error| error.to_string())?
            .is_some()
        {
            let output = child
                .wait_with_output()
                .map_err(|error| error.to_string())?;
            return Ok(output);
        }
        if started.elapsed() >= GIT_TIMEOUT {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!("{} timed out", description));
        }
        thread::sleep(Duration::from_millis(30));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unavailable_checkout_returns_a_bounded_error() {
        let repository = GitRepository {
            path: PathBuf::from("Z:/missing-workflow-repository"),
            branch: "main".to_string(),
        };
        assert!(fetch_and_head(&repository).is_err());
    }
}
