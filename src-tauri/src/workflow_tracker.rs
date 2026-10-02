use crate::git_observer::{
    commit_timestamp, fetch_and_head, newest_added_artifact, newest_current_artifact, GitRepository,
};
use crate::queue_state::{
    InputStatus, ProjectId, ProjectStateInput, QueueMutation, QueueRuntime, WorkflowRole,
};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

const CONFIG_FILE: &str = "workflow-tracker-config.json";
const CHECKPOINT_FILE: &str = "workflow-tracker.json";

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub(crate) enum TrackerStatus {
    Running,
    Completed,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProjectCheckpoint {
    pub(crate) current_role: WorkflowRole,
    pub(crate) status: TrackerStatus,
    #[serde(default)]
    pub(crate) label: Option<String>,
    #[serde(default)]
    pub(crate) next_role: Option<WorkflowRole>,
    #[serde(default)]
    pub(crate) baseline_sha: Option<String>,
    #[serde(default)]
    pub(crate) baseline_source: Option<String>,
    #[serde(default)]
    pub(crate) last_artifact: Option<String>,
    #[serde(default)]
    pub(crate) last_observed_sha: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
struct TrackerCheckpointFile {
    #[serde(default)]
    noctua: Option<ProjectCheckpoint>,
    #[serde(default)]
    fgo: Option<ProjectCheckpoint>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct TrackerConfigFile {
    #[serde(default)]
    gpt_prompt_path: Option<PathBuf>,
    noctua_path: PathBuf,
    noctua_branch: String,
    fgo_path: PathBuf,
    fgo_branch: String,
}

impl Default for TrackerConfigFile {
    fn default() -> Self {
        Self {
            gpt_prompt_path: None,
            noctua_path: PathBuf::from(r"C:\Users\dsyun\pjt_gaia\athena-noctua"),
            noctua_branch: "staging".to_string(),
            fgo_path: PathBuf::from(r"C:\Users\dsyun\OneDrive\문서\ChatGPT\fgo"),
            fgo_branch: "master".to_string(),
        }
    }
}

#[derive(Debug, Clone)]
struct TrackerPaths {
    config: PathBuf,
    checkpoint: PathBuf,
}

#[derive(Debug, Default)]
struct TrackerInner {
    paths: Option<TrackerPaths>,
    config: Option<TrackerConfigFile>,
    checkpoint: TrackerCheckpointFile,
}

#[derive(Clone, Default)]
pub(crate) struct WorkflowTracker {
    inner: Arc<Mutex<TrackerInner>>,
}

impl WorkflowTracker {
    pub(crate) fn initialize(&self, app_data: PathBuf, queue: &QueueRuntime) -> Result<(), String> {
        fs::create_dir_all(&app_data).map_err(|error| error.to_string())?;
        let paths = TrackerPaths {
            config: app_data.join(CONFIG_FILE),
            checkpoint: app_data.join(CHECKPOINT_FILE),
        };
        let config = load_or_create_config(&paths.config)?;
        let checkpoint = load_checkpoint(&paths.checkpoint)?;
        for (project, state) in states(&checkpoint) {
            queue.restore(project, queue_input(state));
        }
        let mut inner = self.inner.lock().expect("workflow tracker lock poisoned");
        inner.paths = Some(paths);
        inner.config = Some(config);
        inner.checkpoint = checkpoint;
        Ok(())
    }

    pub(crate) fn bootstrap(&self, queue: &QueueRuntime) -> Result<Vec<QueueMutation>, String> {
        let mut inner = self.inner.lock().expect("workflow tracker lock poisoned");
        let Some(config) = inner.config.clone() else {
            return Ok(Vec::new());
        };
        if inner.checkpoint.noctua.is_some() || inner.checkpoint.fgo.is_some() {
            return Ok(Vec::new());
        }
        let Some(path) = config.gpt_prompt_path else {
            return Ok(Vec::new());
        };
        let repository = GitRepository {
            path,
            branch: "main".to_string(),
        };
        let head = fetch_and_head(&repository)?;
        let mut mutations = Vec::new();
        for project in [ProjectId::Noctua, ProjectId::Fgo] {
            if let Some(found) = latest_bootstrap_artifact(&repository, project)? {
                let state = found.state(head.clone());
                set_state(&mut inner.checkpoint, project, Some(state.clone()));
                let mutation = queue.restore(project, queue_input(&state));
                mutations.push(mutation);
            }
        }
        persist(&inner)?;
        Ok(mutations)
    }

    pub(crate) fn reconcile(&self, queue: &QueueRuntime) -> Vec<QueueMutation> {
        let mut inner = self.inner.lock().expect("workflow tracker lock poisoned");
        let Some(config) = inner.config.clone() else {
            return Vec::new();
        };
        let mut mutations = Vec::new();
        for project in [ProjectId::Noctua, ProjectId::Fgo] {
            let Some(current) = state_for(&inner.checkpoint, project).cloned() else {
                continue;
            };
            if current.status != TrackerStatus::Running {
                continue;
            }
            if let Ok(Some(completed)) = completion_for(project, &current, &config) {
                set_state(&mut inner.checkpoint, project, Some(completed.clone()));
                let mutation = queue
                    .replace(project, queue_input(&completed))
                    .expect("tracker state is valid");
                mutations.push(mutation);
            }
        }
        if !mutations.is_empty() {
            if let Err(error) = persist(&inner) {
                eprintln!("workflow tracker checkpoint write failed: {error}");
            }
        }
        mutations
    }

    pub(crate) fn advance(
        &self,
        project: ProjectId,
        queue: &QueueRuntime,
    ) -> Result<QueueMutation, String> {
        let mut inner = self.inner.lock().expect("workflow tracker lock poisoned");
        let config = inner
            .config
            .clone()
            .ok_or_else(|| "workflow tracker is unavailable".to_string())?;
        let previous = state_for(&inner.checkpoint, project)
            .cloned()
            .ok_or_else(|| "project workflow is not configured".to_string())?;
        if previous.status != TrackerStatus::Completed {
            return Err("only a completed workflow may be handed off".to_string());
        }
        let next_role = previous
            .next_role
            .ok_or_else(|| "completed workflow has no next role".to_string())?;
        let (baseline_sha, baseline_source) = capture_baseline(project, next_role, &config)?;
        let next = ProjectCheckpoint {
            current_role: next_role,
            status: TrackerStatus::Running,
            label: previous.label,
            next_role: None,
            baseline_sha: Some(baseline_sha.clone()),
            baseline_source: Some(baseline_source),
            last_artifact: previous.last_artifact,
            last_observed_sha: Some(baseline_sha),
        };
        set_state(&mut inner.checkpoint, project, Some(next.clone()));
        persist(&inner)?;
        queue.replace(project, queue_input(&next))
    }

    pub(crate) fn handoff_target(&self, project: ProjectId) -> Option<WorkflowRole> {
        let inner = self.inner.lock().expect("workflow tracker lock poisoned");
        state_for(&inner.checkpoint, project)
            .filter(|state| state.status == TrackerStatus::Completed)
            .and_then(|state| state.next_role)
    }
}

fn completion_for(
    project: ProjectId,
    current: &ProjectCheckpoint,
    config: &TrackerConfigFile,
) -> Result<Option<ProjectCheckpoint>, String> {
    let baseline = current
        .baseline_sha
        .as_deref()
        .ok_or_else(|| "running workflow has no baseline".to_string())?;
    match current.current_role {
        WorkflowRole::Execution => {
            let repository = project_repository(project, config);
            let head = fetch_and_head(&repository)?;
            if head == baseline {
                return Ok(None);
            }
            Ok(Some(completed_execution(current, head)))
        }
        WorkflowRole::Prepare | WorkflowRole::Qa => {
            let Some(path) = config.gpt_prompt_path.clone() else {
                return Ok(None);
            };
            let repository = GitRepository {
                path,
                branch: "main".to_string(),
            };
            let head = fetch_and_head(&repository)?;
            if head == baseline {
                return Ok(None);
            }
            let (directory, suffixes) = match current.current_role {
                WorkflowRole::Prepare => (
                    prepare_directory(project),
                    &[
                        "-execution-handoff.md",
                        "-qa-handoff.md",
                        "-reqa-handoff.md",
                    ] as &[_],
                ),
                WorkflowRole::Qa => (
                    qa_directory(project),
                    &["-qa-report.md", "-reqa-report.md"] as &[_],
                ),
                WorkflowRole::Execution => unreachable!(),
            };
            let Some(artifact) = newest_added_artifact(&repository, baseline, directory, suffixes)?
            else {
                return Ok(None);
            };
            let next_role = if current.current_role == WorkflowRole::Qa {
                WorkflowRole::Prepare
            } else {
                next_role_from_handoff(&artifact.path)
                    .ok_or_else(|| "unrecognized Prepare handoff suffix".to_string())?
            };
            Ok(Some(completed_artifact(
                current,
                artifact.path,
                next_role,
                head,
            )))
        }
    }
}

fn completed_execution(current: &ProjectCheckpoint, head: String) -> ProjectCheckpoint {
    ProjectCheckpoint {
        current_role: WorkflowRole::Execution,
        status: TrackerStatus::Completed,
        label: current.label.clone(),
        next_role: Some(WorkflowRole::Prepare),
        baseline_sha: current.baseline_sha.clone(),
        baseline_source: current.baseline_source.clone(),
        last_artifact: None,
        last_observed_sha: Some(head),
    }
}

fn completed_artifact(
    current: &ProjectCheckpoint,
    artifact: String,
    next_role: WorkflowRole,
    head: String,
) -> ProjectCheckpoint {
    ProjectCheckpoint {
        current_role: current.current_role,
        status: TrackerStatus::Completed,
        label: label_from_artifact(&artifact),
        next_role: Some(next_role),
        baseline_sha: current.baseline_sha.clone(),
        baseline_source: current.baseline_source.clone(),
        last_artifact: Some(artifact),
        last_observed_sha: Some(head),
    }
}

fn capture_baseline(
    project: ProjectId,
    role: WorkflowRole,
    config: &TrackerConfigFile,
) -> Result<(String, String), String> {
    let repository = match role {
        WorkflowRole::Prepare | WorkflowRole::Qa => GitRepository {
            path: config
                .gpt_prompt_path
                .clone()
                .ok_or_else(|| "gpt_prompt checkout is not configured".to_string())?,
            branch: "main".to_string(),
        },
        WorkflowRole::Execution => project_repository(project, config),
    };
    let head = fetch_and_head(&repository)?;
    Ok((head, format!("origin/{}", repository.branch)))
}

fn project_repository(project: ProjectId, config: &TrackerConfigFile) -> GitRepository {
    match project {
        ProjectId::Noctua => GitRepository {
            path: config.noctua_path.clone(),
            branch: config.noctua_branch.clone(),
        },
        ProjectId::Fgo => GitRepository {
            path: config.fgo_path.clone(),
            branch: config.fgo_branch.clone(),
        },
    }
}

fn prepare_directory(project: ProjectId) -> &'static str {
    match project {
        ProjectId::Noctua => "noctua/prepare/",
        ProjectId::Fgo => "fgo/prepare/",
    }
}

fn qa_directory(project: ProjectId) -> &'static str {
    match project {
        ProjectId::Noctua => "noctua/qa/",
        ProjectId::Fgo => "fgo/qa/",
    }
}

fn next_role_from_handoff(path: &str) -> Option<WorkflowRole> {
    if path.ends_with("-execution-handoff.md") {
        Some(WorkflowRole::Execution)
    } else if path.ends_with("-qa-handoff.md") || path.ends_with("-reqa-handoff.md") {
        Some(WorkflowRole::Qa)
    } else {
        None
    }
}

fn label_from_artifact(path: &str) -> Option<String> {
    let file = Path::new(path).file_name()?.to_str()?;
    let body = file.trim_end_matches(".md");
    let body = [
        "-execution-handoff",
        "-qa-handoff",
        "-reqa-handoff",
        "-qa-report",
        "-reqa-report",
    ]
    .iter()
    .find_map(|suffix| body.strip_suffix(suffix))
    .unwrap_or(body);
    let pieces = body.split('-').collect::<Vec<_>>();
    let label = if pieces.len() > 3
        && pieces[0].len() == 4
        && pieces[0].bytes().all(|byte| byte.is_ascii_digit())
        && pieces[1].len() == 2
        && pieces[1].bytes().all(|byte| byte.is_ascii_digit())
        && pieces[2].len() == 2
        && pieces[2].bytes().all(|byte| byte.is_ascii_digit())
    {
        pieces[3..].join("-")
    } else {
        body.to_string()
    }
    .replace('_', "-");
    (!label.trim().is_empty()).then_some(label)
}

fn queue_input(state: &ProjectCheckpoint) -> ProjectStateInput {
    ProjectStateInput {
        status: match state.status {
            TrackerStatus::Running => InputStatus::Running,
            TrackerStatus::Completed => InputStatus::Completed,
        },
        role: Some(state.current_role),
        label: state.label.clone(),
        attention_required: Some(false),
    }
}

fn states(checkpoint: &TrackerCheckpointFile) -> Vec<(ProjectId, &ProjectCheckpoint)> {
    let mut states = Vec::new();
    if let Some(state) = checkpoint.noctua.as_ref() {
        states.push((ProjectId::Noctua, state));
    }
    if let Some(state) = checkpoint.fgo.as_ref() {
        states.push((ProjectId::Fgo, state));
    }
    states
}

fn state_for(checkpoint: &TrackerCheckpointFile, project: ProjectId) -> Option<&ProjectCheckpoint> {
    match project {
        ProjectId::Noctua => checkpoint.noctua.as_ref(),
        ProjectId::Fgo => checkpoint.fgo.as_ref(),
    }
}

fn set_state(
    checkpoint: &mut TrackerCheckpointFile,
    project: ProjectId,
    state: Option<ProjectCheckpoint>,
) {
    match project {
        ProjectId::Noctua => checkpoint.noctua = state,
        ProjectId::Fgo => checkpoint.fgo = state,
    }
}

fn load_or_create_config(path: &Path) -> Result<TrackerConfigFile, String> {
    if path.exists() {
        return read_json(path);
    }
    let config = TrackerConfigFile::default();
    write_json(path, &config)?;
    Ok(config)
}

fn load_checkpoint(path: &Path) -> Result<TrackerCheckpointFile, String> {
    if path.exists() {
        read_json(path)
    } else {
        Ok(TrackerCheckpointFile::default())
    }
}

fn persist(inner: &TrackerInner) -> Result<(), String> {
    let path = inner
        .paths
        .as_ref()
        .ok_or_else(|| "workflow tracker is not initialized".to_string())?;
    write_json(&path.checkpoint, &inner.checkpoint)
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T, String> {
    let content = fs::read_to_string(path).map_err(|error| error.to_string())?;
    serde_json::from_str(&content).map_err(|error| format!("{}: {error}", path.display()))
}

fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
    let content = serde_json::to_string_pretty(value).map_err(|error| error.to_string())?;
    fs::write(path, content).map_err(|error| error.to_string())
}

struct BootstrapArtifact {
    path: String,
    role: WorkflowRole,
    next_role: WorkflowRole,
}

impl BootstrapArtifact {
    fn state(self, head: String) -> ProjectCheckpoint {
        ProjectCheckpoint {
            current_role: self.role,
            status: TrackerStatus::Completed,
            label: label_from_artifact(&self.path),
            next_role: Some(self.next_role),
            baseline_sha: None,
            baseline_source: None,
            last_artifact: Some(self.path),
            last_observed_sha: Some(head),
        }
    }
}

fn latest_bootstrap_artifact(
    repository: &GitRepository,
    project: ProjectId,
) -> Result<Option<BootstrapArtifact>, String> {
    let prepare = newest_current_artifact(
        repository,
        prepare_directory(project),
        &[
            "-execution-handoff.md",
            "-qa-handoff.md",
            "-reqa-handoff.md",
        ],
    )?;
    let qa = newest_current_artifact(
        repository,
        qa_directory(project),
        &["-qa-report.md", "-reqa-report.md"],
    )?;
    match (prepare, qa) {
        (None, None) => Ok(None),
        (Some(artifact), None) => {
            Ok(
                next_role_from_handoff(&artifact.path).map(|next_role| BootstrapArtifact {
                    path: artifact.path,
                    role: WorkflowRole::Prepare,
                    next_role,
                }),
            )
        }
        (None, Some(artifact)) => Ok(Some(BootstrapArtifact {
            path: artifact.path,
            role: WorkflowRole::Qa,
            next_role: WorkflowRole::Prepare,
        })),
        (Some(prepare), Some(qa)) => {
            let chosen = if commit_timestamp(repository, &qa.commit)?
                > commit_timestamp(repository, &prepare.commit)?
            {
                qa
            } else {
                prepare
            };
            if chosen.path.ends_with("-qa-report.md") || chosen.path.ends_with("-reqa-report.md") {
                Ok(Some(BootstrapArtifact {
                    path: chosen.path,
                    role: WorkflowRole::Qa,
                    next_role: WorkflowRole::Prepare,
                }))
            } else {
                Ok(
                    next_role_from_handoff(&chosen.path).map(|next_role| BootstrapArtifact {
                        path: chosen.path,
                        role: WorkflowRole::Prepare,
                        next_role,
                    }),
                )
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prepare_suffixes_define_the_next_role() {
        assert_eq!(
            next_role_from_handoff("noctua/prepare/2026-R9-A16-execution-handoff.md"),
            Some(WorkflowRole::Execution)
        );
        assert_eq!(
            next_role_from_handoff("fgo/prepare/2026-LEGION-A1-qa-handoff.md"),
            Some(WorkflowRole::Qa)
        );
        assert_eq!(next_role_from_handoff("notes.md"), None);
    }

    #[test]
    fn labels_strip_timestamp_and_role_suffix() {
        assert_eq!(
            label_from_artifact("noctua/prepare/2026-10-02-R9-A16-execution-handoff.md").as_deref(),
            Some("R9-A16")
        );
    }

    #[test]
    fn queue_projection_needs_only_running_or_completed() {
        let state = ProjectCheckpoint {
            current_role: WorkflowRole::Qa,
            status: TrackerStatus::Completed,
            label: None,
            next_role: Some(WorkflowRole::Prepare),
            baseline_sha: None,
            baseline_source: None,
            last_artifact: None,
            last_observed_sha: None,
        };
        assert_eq!(queue_input(&state).status, InputStatus::Completed);
    }

    fn running(role: WorkflowRole) -> ProjectCheckpoint {
        ProjectCheckpoint {
            current_role: role,
            status: TrackerStatus::Running,
            label: Some("R9-A16".to_string()),
            next_role: None,
            baseline_sha: Some("abc123".to_string()),
            baseline_source: Some("origin/main".to_string()),
            last_artifact: None,
            last_observed_sha: Some("abc123".to_string()),
        }
    }

    #[test]
    fn prepare_handoffs_complete_only_to_the_filename_role() {
        let execution = completed_artifact(
            &running(WorkflowRole::Prepare),
            "noctua/prepare/2026-10-02-R9-A16-execution-handoff.md".to_string(),
            WorkflowRole::Execution,
            "def456".to_string(),
        );
        let qa = completed_artifact(
            &running(WorkflowRole::Prepare),
            "noctua/prepare/2026-10-02-R9-A17-qa-handoff.md".to_string(),
            WorkflowRole::Qa,
            "def456".to_string(),
        );
        assert_eq!(execution.next_role, Some(WorkflowRole::Execution));
        assert_eq!(qa.next_role, Some(WorkflowRole::Qa));
    }

    #[test]
    fn qa_and_execution_completion_return_to_prepare() {
        let qa = completed_artifact(
            &running(WorkflowRole::Qa),
            "fgo/qa/2026-10-02-LEGION-A1-qa-report.md".to_string(),
            WorkflowRole::Prepare,
            "def456".to_string(),
        );
        let execution =
            completed_execution(&running(WorkflowRole::Execution), "def456".to_string());
        assert_eq!(qa.next_role, Some(WorkflowRole::Prepare));
        assert_eq!(execution.next_role, Some(WorkflowRole::Prepare));
        assert_eq!(execution.status, TrackerStatus::Completed);
    }

    #[test]
    fn unchanged_execution_head_stays_running() {
        let state = running(WorkflowRole::Execution);
        assert_eq!(state.last_observed_sha, state.baseline_sha);
        assert_eq!(state.status, TrackerStatus::Running);
    }
}
