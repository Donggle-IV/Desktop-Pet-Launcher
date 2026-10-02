use crate::git_observer::{
    commit_timestamp, fetch_and_head, fetch_head_with_timestamp, is_ancestor,
    newest_added_artifact, newest_current_artifact, GitArtifact, GitRepository,
};
use crate::queue_state::{
    InputStatus, ProjectId, ProjectStateInput, QueueMutation, QueueRuntime, WorkflowRole,
};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

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
    pub(crate) last_artifact_commit: Option<String>,
    #[serde(default)]
    pub(crate) last_observed_sha: Option<String>,
    /// Local user-confirmation time for an Execution completion without Git evidence.
    #[serde(default)]
    pub(crate) manual_completed_at: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TrackerProjectView {
    pub(crate) role: WorkflowRole,
    pub(crate) status: TrackerStatus,
    pub(crate) label: Option<String>,
    pub(crate) next_role: Option<WorkflowRole>,
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
    pub(crate) fn initialize(&self, app_data: PathBuf) -> Result<(), String> {
        fs::create_dir_all(&app_data).map_err(|error| error.to_string())?;
        let paths = TrackerPaths {
            config: app_data.join(CONFIG_FILE),
            checkpoint: app_data.join(CHECKPOINT_FILE),
        };
        let config = load_or_create_config(&paths.config)?;
        let checkpoint = load_checkpoint(&paths.checkpoint)?;
        let mut inner = self.inner.lock().expect("workflow tracker lock poisoned");
        inner.paths = Some(paths);
        inner.config = Some(config);
        inner.checkpoint = checkpoint;
        Ok(())
    }

    pub(crate) fn startup_reconcile(&self, queue: &QueueRuntime) -> Vec<QueueMutation> {
        let mut inner = self.inner.lock().expect("workflow tracker lock poisoned");
        let Some(config) = inner.config.clone() else {
            return Vec::new();
        };
        let mut mutations = Vec::new();
        for project in [ProjectId::Noctua, ProjectId::Fgo] {
            let persisted = state_for(&inner.checkpoint, project).cloned();
            match startup_state_for(project, persisted.as_ref(), &config) {
                Ok(Some(state)) => {
                    set_state(&mut inner.checkpoint, project, Some(state.clone()));
                    mutations.push(queue.restore(project, queue_input(&state)));
                }
                Ok(None) => {
                    if let Some(state) = persisted {
                        mutations.push(queue.restore(project, queue_input(&state)));
                    }
                }
                Err(error) => {
                    eprintln!("workflow tracker startup reconciliation for {project:?} retained its state: {error}");
                    if let Some(state) = persisted {
                        mutations.push(queue.restore(project, queue_input(&state)));
                    }
                }
            }
        }
        if let Err(error) = persist(&inner) {
            eprintln!("workflow tracker checkpoint write failed: {error}");
        }
        mutations
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
            match completion_for(project, &current, &config) {
                Ok(Some(completed)) => {
                    set_state(&mut inner.checkpoint, project, Some(completed.clone()));
                    let mutation = queue
                        .replace(project, queue_input(&completed))
                        .expect("tracker state is valid");
                    mutations.push(mutation);
                }
                Ok(None) => {}
                Err(error) => eprintln!(
                    "workflow tracker reconciliation for {project:?} retained its state: {error}"
                ),
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
        let (baseline_sha, baseline_source) =
            transition_baseline(project, next_role, &previous, &config)?;
        let next = ProjectCheckpoint {
            current_role: next_role,
            status: TrackerStatus::Running,
            label: previous.label,
            next_role: None,
            baseline_sha: Some(baseline_sha.clone()),
            baseline_source: Some(baseline_source),
            last_artifact: previous.last_artifact,
            last_artifact_commit: previous.last_artifact_commit,
            last_observed_sha: Some(baseline_sha),
            manual_completed_at: None,
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

    pub(crate) fn view(&self, project: ProjectId) -> Option<TrackerProjectView> {
        let inner = self.inner.lock().expect("workflow tracker lock poisoned");
        state_for(&inner.checkpoint, project).map(|state| TrackerProjectView {
            role: state.current_role,
            status: state.status,
            label: state.label.clone(),
            next_role: state.next_role,
        })
    }

    pub(crate) fn align(
        &self,
        project: ProjectId,
        role: WorkflowRole,
        status: TrackerStatus,
        next_role: Option<WorkflowRole>,
        queue: &QueueRuntime,
    ) -> Result<QueueMutation, String> {
        let mut inner = self.inner.lock().expect("workflow tracker lock poisoned");
        let config = inner
            .config
            .clone()
            .ok_or_else(|| "workflow tracker is unavailable".to_string())?;
        let prior = state_for(&inner.checkpoint, project).cloned();
        let required_next = match (role, status) {
            (_, TrackerStatus::Running) => None,
            (WorkflowRole::Qa | WorkflowRole::Execution, TrackerStatus::Completed) => {
                Some(WorkflowRole::Prepare)
            }
            (WorkflowRole::Prepare, TrackerStatus::Completed) => {
                Some(next_role.ok_or_else(|| {
                    "Prepare completed alignment requires a next role".to_string()
                })?)
            }
        };
        if status == TrackerStatus::Completed
            && role != WorkflowRole::Prepare
            && next_role.is_some()
            && next_role != required_next
        {
            return Err("completed QA and Execution always hand off to Prepare".to_string());
        }
        let (baseline_sha, baseline_source, observed_sha) = if status == TrackerStatus::Running {
            let (sha, source) = capture_baseline(project, role, &config)?;
            (Some(sha.clone()), Some(source), Some(sha))
        } else {
            (
                prior.as_ref().and_then(|state| state.baseline_sha.clone()),
                prior
                    .as_ref()
                    .and_then(|state| state.baseline_source.clone()),
                prior
                    .as_ref()
                    .and_then(|state| state.last_observed_sha.clone()),
            )
        };
        let next = ProjectCheckpoint {
            current_role: role,
            status,
            label: prior.as_ref().and_then(|state| state.label.clone()),
            next_role: required_next,
            baseline_sha,
            baseline_source,
            last_artifact: prior.as_ref().and_then(|state| state.last_artifact.clone()),
            last_artifact_commit: prior
                .as_ref()
                .and_then(|state| state.last_artifact_commit.clone()),
            last_observed_sha: observed_sha,
            manual_completed_at: None,
        };
        set_state(&mut inner.checkpoint, project, Some(next.clone()));
        persist(&inner)?;
        Ok(queue.restore(project, queue_input(&next)))
    }

    pub(crate) fn complete_execution(
        &self,
        project: ProjectId,
        queue: &QueueRuntime,
    ) -> Result<QueueMutation, String> {
        let mut inner = self.inner.lock().expect("workflow tracker lock poisoned");
        let current = state_for(&inner.checkpoint, project)
            .cloned()
            .ok_or_else(|| "project workflow is not configured".to_string())?;
        validate_manual_execution_completion(&current)?;
        let completed = manually_completed_execution(&current, unix_time_millis());
        set_state(&mut inner.checkpoint, project, Some(completed.clone()));
        persist(&inner)?;
        queue.replace(project, queue_input(&completed))
    }
}

fn validate_manual_execution_completion(current: &ProjectCheckpoint) -> Result<(), String> {
    if current.current_role != WorkflowRole::Execution || current.status != TrackerStatus::Running {
        return Err("only a running Execution workflow may be manually completed".to_string());
    }
    Ok(())
}

fn startup_state_for(
    project: ProjectId,
    persisted: Option<&ProjectCheckpoint>,
    config: &TrackerConfigFile,
) -> Result<Option<ProjectCheckpoint>, String> {
    match persisted {
        None => latest_completed_state(project, config),
        Some(state) if state.status == TrackerStatus::Running => {
            Ok(completion_for(project, state, config)?.or_else(|| Some(state.clone())))
        }
        Some(state) => reconcile_completed_startup(project, state, config),
    }
}

fn reconcile_completed_startup(
    project: ProjectId,
    persisted: &ProjectCheckpoint,
    config: &TrackerConfigFile,
) -> Result<Option<ProjectCheckpoint>, String> {
    let persisted_timestamp = completed_timestamp(project, persisted, config)?;
    let Some(current) = latest_completed_evidence(project, config)? else {
        eprintln!("workflow tracker startup reconciliation for {project:?} found ambiguous completion evidence");
        return Ok(Some(persisted.clone()));
    };
    Ok(Some(newer_completed_state(
        persisted,
        persisted_timestamp,
        current,
    )))
}

fn newer_completed_state(
    persisted: &ProjectCheckpoint,
    persisted_timestamp: u64,
    current: BootstrapEvidence,
) -> ProjectCheckpoint {
    if current.evidence_timestamp > persisted_timestamp {
        current.state()
    } else {
        persisted.clone()
    }
}

fn completed_timestamp(
    project: ProjectId,
    state: &ProjectCheckpoint,
    config: &TrackerConfigFile,
) -> Result<u64, String> {
    match state.current_role {
        WorkflowRole::Prepare | WorkflowRole::Qa => {
            let commit = state.last_artifact_commit.as_deref().ok_or_else(|| {
                "completed gpt_prompt workflow has no artifact commit identity; use Settings alignment"
                    .to_string()
            })?;
            let repository = gpt_prompt_repository(config)?;
            fetch_and_head(&repository)?;
            git_timestamp_millis(commit_timestamp(&repository, commit)?)
        }
        WorkflowRole::Execution => {
            if let Some(timestamp) = state.manual_completed_at {
                return Ok(timestamp);
            }
            let commit = state.last_observed_sha.as_deref().ok_or_else(|| {
                "completed Execution workflow has no observed commit identity; use Settings alignment"
                    .to_string()
            })?;
            let repository = project_repository(project, config);
            fetch_and_head(&repository)?;
            git_timestamp_millis(commit_timestamp(&repository, commit)?)
        }
    }
}

fn latest_completed_state(
    project: ProjectId,
    config: &TrackerConfigFile,
) -> Result<Option<ProjectCheckpoint>, String> {
    Ok(latest_completed_evidence(project, config)?.map(BootstrapEvidence::state))
}

fn git_timestamp_millis(timestamp_seconds: u64) -> Result<u64, String> {
    timestamp_seconds
        .checked_mul(1_000)
        .ok_or_else(|| "Git commit timestamp is too large".to_string())
}

fn unix_time_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
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
            if !is_ancestor(&repository, baseline)? {
                return Err("stored gpt_prompt baseline is no longer an ancestor of origin/main; use Settings alignment".to_string());
            }
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
            Ok(Some(completed_artifact(current, artifact, next_role, head)))
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
        last_artifact: current.last_artifact.clone(),
        last_artifact_commit: current.last_artifact_commit.clone(),
        last_observed_sha: Some(head),
        manual_completed_at: None,
    }
}

fn manually_completed_execution(current: &ProjectCheckpoint, timestamp: u64) -> ProjectCheckpoint {
    ProjectCheckpoint {
        current_role: WorkflowRole::Execution,
        status: TrackerStatus::Completed,
        label: current.label.clone(),
        next_role: Some(WorkflowRole::Prepare),
        baseline_sha: current.baseline_sha.clone(),
        baseline_source: current.baseline_source.clone(),
        last_artifact: current.last_artifact.clone(),
        last_artifact_commit: current.last_artifact_commit.clone(),
        last_observed_sha: current.last_observed_sha.clone(),
        manual_completed_at: Some(timestamp),
    }
}

fn completed_artifact(
    current: &ProjectCheckpoint,
    artifact: GitArtifact,
    next_role: WorkflowRole,
    head: String,
) -> ProjectCheckpoint {
    ProjectCheckpoint {
        current_role: current.current_role,
        status: TrackerStatus::Completed,
        label: label_from_artifact(&artifact.path),
        next_role: Some(next_role),
        baseline_sha: current.baseline_sha.clone(),
        baseline_source: current.baseline_source.clone(),
        last_artifact: Some(artifact.path),
        last_artifact_commit: Some(artifact.commit),
        last_observed_sha: Some(head),
        manual_completed_at: None,
    }
}

fn transition_baseline(
    project: ProjectId,
    next_role: WorkflowRole,
    previous: &ProjectCheckpoint,
    config: &TrackerConfigFile,
) -> Result<(String, String), String> {
    if next_role == WorkflowRole::Execution {
        return capture_baseline(project, next_role, config);
    }
    let anchor = artifact_anchor_for_transition(next_role, previous)?;
    let repository = GitRepository {
        path: config
            .gpt_prompt_path
            .clone()
            .ok_or_else(|| "gpt_prompt checkout is not configured".to_string())?,
        branch: "main".to_string(),
    };
    fetch_and_head(&repository)?;
    if !is_ancestor(&repository, anchor)? {
        return Err("stored gpt_prompt artifact anchor is no longer an ancestor of origin/main; use Settings alignment".to_string());
    }
    Ok((anchor.to_string(), "origin/main".to_string()))
}

fn artifact_anchor_for_transition(
    next_role: WorkflowRole,
    previous: &ProjectCheckpoint,
) -> Result<&str, String> {
    if next_role == WorkflowRole::Execution {
        return Err("Execution uses its target project remote baseline".to_string());
    }
    previous.last_artifact_commit.as_deref().ok_or_else(|| {
        "completed workflow has no causal gpt_prompt artifact anchor; use Settings alignment"
            .to_string()
    })
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

fn gpt_prompt_repository(config: &TrackerConfigFile) -> Result<GitRepository, String> {
    Ok(GitRepository {
        path: config
            .gpt_prompt_path
            .clone()
            .ok_or_else(|| "gpt_prompt checkout is not configured".to_string())?,
        branch: "main".to_string(),
    })
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
    let normalized = body.replace('_', "-");
    let pieces = normalized.split('-').collect::<Vec<_>>();
    let start = if pieces.len() >= 3
        && pieces[0].len() == 8
        && pieces[0].bytes().all(|byte| byte.is_ascii_digit())
        && pieces[1].len() == 6
        && pieces[1].bytes().all(|byte| byte.is_ascii_digit())
    {
        2
    } else {
        0
    };
    let meaningful = &pieces[start..];
    let label = if meaningful.len() >= 2
        && meaningful[0].starts_with('r')
        && meaningful[0][1..].bytes().all(|byte| byte.is_ascii_digit())
        && meaningful[1].starts_with('a')
        && meaningful[1][1..].bytes().all(|byte| byte.is_ascii_digit())
    {
        let mut parts = vec![meaningful[0].to_uppercase(), meaningful[1].to_uppercase()];
        if meaningful.get(2).is_some_and(|part| {
            part.starts_with('h') && part[1..].bytes().all(|byte| byte.is_ascii_digit())
        }) {
            parts.push(meaningful[2].to_uppercase());
        }
        parts.join("-")
    } else {
        meaningful
            .iter()
            .take(2)
            .map(|part| part.to_uppercase())
            .collect::<Vec<_>>()
            .join("-")
    };
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

#[derive(Debug, Clone)]
struct BootstrapEvidence {
    role: WorkflowRole,
    next_role: WorkflowRole,
    evidence_sha: String,
    evidence_timestamp: u64,
    artifact: Option<GitArtifact>,
}

impl BootstrapEvidence {
    fn state(self) -> ProjectCheckpoint {
        let label = self
            .artifact
            .as_ref()
            .and_then(|artifact| label_from_artifact(&artifact.path));
        let last_artifact = self.artifact.as_ref().map(|artifact| artifact.path.clone());
        let last_artifact_commit = self
            .artifact
            .as_ref()
            .map(|artifact| artifact.commit.clone());
        ProjectCheckpoint {
            current_role: self.role,
            status: TrackerStatus::Completed,
            label,
            next_role: Some(self.next_role),
            baseline_sha: None,
            baseline_source: None,
            last_artifact,
            last_artifact_commit,
            last_observed_sha: Some(self.evidence_sha),
            manual_completed_at: None,
        }
    }
}

fn latest_completed_evidence(
    project: ProjectId,
    config: &TrackerConfigFile,
) -> Result<Option<BootstrapEvidence>, String> {
    let gpt_prompt = gpt_prompt_repository(config)?;
    fetch_and_head(&gpt_prompt)?;
    let target = project_repository(project, config);
    let prepare = newest_current_artifact(
        &gpt_prompt,
        prepare_directory(project),
        &[
            "-execution-handoff.md",
            "-qa-handoff.md",
            "-reqa-handoff.md",
        ],
    )?;
    let qa = newest_current_artifact(
        &gpt_prompt,
        qa_directory(project),
        &["-qa-report.md", "-reqa-report.md"],
    )?;
    let mut candidates = Vec::new();
    if let Some(artifact) = prepare {
        if let Some(next_role) = next_role_from_handoff(&artifact.path) {
            candidates.push(BootstrapEvidence {
                role: WorkflowRole::Prepare,
                next_role,
                evidence_sha: artifact.commit.clone(),
                evidence_timestamp: git_timestamp_millis(commit_timestamp(
                    &gpt_prompt,
                    &artifact.commit,
                )?)?,
                artifact: Some(artifact),
            });
        }
    }
    if let Some(artifact) = qa {
        candidates.push(BootstrapEvidence {
            role: WorkflowRole::Qa,
            next_role: WorkflowRole::Prepare,
            evidence_sha: artifact.commit.clone(),
            evidence_timestamp: git_timestamp_millis(commit_timestamp(
                &gpt_prompt,
                &artifact.commit,
            )?)?,
            artifact: Some(artifact),
        });
    }
    let (execution_head, execution_timestamp_seconds) = fetch_head_with_timestamp(&target)?;
    let execution_timestamp = git_timestamp_millis(execution_timestamp_seconds)?;
    let execution_anchor = candidates
        .iter()
        .filter(|candidate| {
            candidate.role == WorkflowRole::Prepare
                && candidate.next_role == WorkflowRole::Execution
        })
        .filter(|candidate| candidate.evidence_timestamp <= execution_timestamp)
        .max_by_key(|candidate| candidate.evidence_timestamp)
        .and_then(|candidate| candidate.artifact.clone());
    candidates.push(BootstrapEvidence {
        role: WorkflowRole::Execution,
        next_role: WorkflowRole::Prepare,
        evidence_sha: execution_head,
        evidence_timestamp: execution_timestamp,
        artifact: execution_anchor,
    });
    Ok(select_bootstrap(candidates))
}

fn select_bootstrap(candidates: Vec<BootstrapEvidence>) -> Option<BootstrapEvidence> {
    let latest = candidates
        .iter()
        .map(|candidate| candidate.evidence_timestamp)
        .max()?;
    let mut latest_candidates = candidates
        .into_iter()
        .filter(|candidate| candidate.evidence_timestamp == latest);
    let selected = latest_candidates.next()?;
    latest_candidates.next().is_none().then_some(selected)
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
            label_from_artifact("noctua/prepare/20261002-124800-r9-a16-final-pre-qa-contract-completion-execution-handoff.md").as_deref(),
            Some("R9-A16")
        );
        assert_eq!(
            label_from_artifact("20261001-204100-r9-a8-h01-session-binding-execution-handoff.md")
                .as_deref(),
            Some("R9-A8-H01")
        );
        assert_eq!(
            label_from_artifact(
                "20261002-124500-legion-proc-maps-libil2cpp-load-bias-execution-handoff.md"
            )
            .as_deref(),
            Some("LEGION-PROC")
        );
        assert_eq!(
            label_from_artifact("20261002-130702-committed-mapping-elf-correlation-qa-handoff.md")
                .as_deref(),
            Some("COMMITTED-MAPPING")
        );
        assert_eq!(
            label_from_artifact(
                "20261002-121035-host-lldb-source-correlation-execution-handoff.md"
            )
            .as_deref(),
            Some("HOST-LLDB")
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
            last_artifact_commit: None,
            last_observed_sha: None,
            manual_completed_at: None,
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
            last_artifact_commit: None,
            last_observed_sha: Some("abc123".to_string()),
            manual_completed_at: None,
        }
    }

    fn artifact(path: &str, commit: &str) -> GitArtifact {
        GitArtifact {
            path: path.to_string(),
            commit: commit.to_string(),
        }
    }

    #[test]
    fn prepare_handoffs_complete_only_to_the_filename_role() {
        let execution = completed_artifact(
            &running(WorkflowRole::Prepare),
            artifact(
                "noctua/prepare/2026-10-02-R9-A16-execution-handoff.md",
                "prepare-execution",
            ),
            WorkflowRole::Execution,
            "def456".to_string(),
        );
        let qa = completed_artifact(
            &running(WorkflowRole::Prepare),
            artifact(
                "noctua/prepare/2026-10-02-R9-A17-qa-handoff.md",
                "prepare-qa",
            ),
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
            artifact("fgo/qa/2026-10-02-LEGION-A1-qa-report.md", "qa-report"),
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

    #[test]
    fn causal_handoffs_use_the_completion_artifact_not_click_time_head() {
        let prepare_to_qa = completed_artifact(
            &running(WorkflowRole::Prepare),
            artifact("noctua/prepare/p1-qa-handoff.md", "P1"),
            WorkflowRole::Qa,
            "gpt-head-after-q1".to_string(),
        );
        let qa_to_prepare = completed_artifact(
            &running(WorkflowRole::Qa),
            artifact("noctua/qa/q1-qa-report.md", "Q1"),
            WorkflowRole::Prepare,
            "gpt-head-after-p2".to_string(),
        );
        let execution_to_prepare = completed_execution(
            &completed_artifact(
                &running(WorkflowRole::Prepare),
                artifact("noctua/prepare/p1-execution-handoff.md", "P1E"),
                WorkflowRole::Execution,
                "gpt-head".to_string(),
            ),
            "E1".to_string(),
        );
        assert_eq!(
            artifact_anchor_for_transition(WorkflowRole::Qa, &prepare_to_qa).unwrap(),
            "P1"
        );
        assert_eq!(
            artifact_anchor_for_transition(WorkflowRole::Prepare, &qa_to_prepare).unwrap(),
            "Q1"
        );
        assert_eq!(
            artifact_anchor_for_transition(WorkflowRole::Prepare, &execution_to_prepare).unwrap(),
            "P1E"
        );
        assert_eq!(
            execution_to_prepare.last_artifact.as_deref(),
            Some("noctua/prepare/p1-execution-handoff.md")
        );
    }

    fn evidence(role: WorkflowRole, timestamp: u64) -> BootstrapEvidence {
        BootstrapEvidence {
            role,
            next_role: WorkflowRole::Prepare,
            evidence_sha: format!("{role:?}-{timestamp}"),
            evidence_timestamp: timestamp,
            artifact: None,
        }
    }

    #[test]
    fn bootstrap_selects_only_a_unique_latest_commit_timestamp() {
        assert_eq!(
            select_bootstrap(vec![
                evidence(WorkflowRole::Prepare, 30),
                evidence(WorkflowRole::Qa, 20),
                evidence(WorkflowRole::Execution, 10),
            ])
            .unwrap()
            .role,
            WorkflowRole::Prepare
        );
        assert_eq!(
            select_bootstrap(vec![
                evidence(WorkflowRole::Prepare, 20),
                evidence(WorkflowRole::Qa, 30),
                evidence(WorkflowRole::Execution, 10),
            ])
            .unwrap()
            .role,
            WorkflowRole::Qa
        );
        assert_eq!(
            select_bootstrap(vec![
                evidence(WorkflowRole::Prepare, 20),
                evidence(WorkflowRole::Qa, 10),
                evidence(WorkflowRole::Execution, 30),
            ])
            .unwrap()
            .role,
            WorkflowRole::Execution
        );
        assert!(select_bootstrap(vec![
            evidence(WorkflowRole::Prepare, 30),
            evidence(WorkflowRole::Qa, 30)
        ])
        .is_none());
        assert_eq!(
            select_bootstrap(vec![evidence(WorkflowRole::Qa, 30)])
                .unwrap()
                .role,
            WorkflowRole::Qa
        );
        assert!(select_bootstrap(Vec::new()).is_none());
    }

    #[test]
    fn execution_bootstrap_keeps_its_preceding_prepare_anchor() {
        let execution = BootstrapEvidence {
            role: WorkflowRole::Execution,
            next_role: WorkflowRole::Prepare,
            evidence_sha: "E1".to_string(),
            evidence_timestamp: 30,
            artifact: Some(artifact("fgo/prepare/p1-execution-handoff.md", "P1")),
        };
        let state = select_bootstrap(vec![evidence(WorkflowRole::Qa, 20), execution])
            .unwrap()
            .state();
        assert_eq!(state.current_role, WorkflowRole::Execution);
        assert_eq!(state.last_artifact_commit.as_deref(), Some("P1"));
        assert_eq!(state.label.as_deref(), Some("P1"));
    }

    #[test]
    fn startup_completed_checkpoint_catches_up_only_to_newer_unique_evidence() {
        let persisted = completed_artifact(
            &running(WorkflowRole::Prepare),
            artifact("noctua/prepare/p1-execution-handoff.md", "P1"),
            WorkflowRole::Execution,
            "P1".to_string(),
        );
        let execution = BootstrapEvidence {
            role: WorkflowRole::Execution,
            next_role: WorkflowRole::Prepare,
            evidence_sha: "E2".to_string(),
            evidence_timestamp: 20,
            artifact: Some(artifact("noctua/prepare/p1-execution-handoff.md", "P1")),
        };
        assert_eq!(
            newer_completed_state(&persisted, 10, execution).current_role,
            WorkflowRole::Execution
        );
        assert_eq!(
            newer_completed_state(&persisted, 20, evidence(WorkflowRole::Qa, 20)).current_role,
            WorkflowRole::Prepare
        );
    }

    #[test]
    fn startup_recovery_restore_does_not_emit_completion() {
        let queue = QueueRuntime::default();
        let completed = completed_artifact(
            &running(WorkflowRole::Qa),
            artifact("fgo/qa/q1-qa-report.md", "Q1"),
            WorkflowRole::Prepare,
            "head".to_string(),
        );
        assert!(queue
            .restore(ProjectId::Fgo, queue_input(&completed))
            .completed
            .is_none());
    }

    #[test]
    fn manual_execution_completion_validates_role_and_preserves_anchor() {
        let mut execution = running(WorkflowRole::Execution);
        execution.last_artifact = Some("noctua/prepare/p1-execution-handoff.md".to_string());
        execution.last_artifact_commit = Some("P1".to_string());
        execution.last_observed_sha = Some("E0".to_string());
        assert!(validate_manual_execution_completion(&execution).is_ok());
        let completed = manually_completed_execution(&execution, 1_420_000);
        assert_eq!(completed.status, TrackerStatus::Completed);
        assert_eq!(completed.next_role, Some(WorkflowRole::Prepare));
        assert_eq!(completed.last_artifact_commit.as_deref(), Some("P1"));
        assert_eq!(completed.manual_completed_at, Some(1_420_000));
        assert_eq!(
            artifact_anchor_for_transition(WorkflowRole::Prepare, &completed).unwrap(),
            "P1"
        );
        let queue = QueueRuntime::default();
        queue.restore(ProjectId::Noctua, queue_input(&execution));
        assert!(queue
            .replace(ProjectId::Noctua, queue_input(&completed))
            .unwrap()
            .completed
            .is_some());
        assert!(validate_manual_execution_completion(&completed).is_err());
        assert!(validate_manual_execution_completion(&running(WorkflowRole::Prepare)).is_err());
        assert!(validate_manual_execution_completion(&running(WorkflowRole::Qa)).is_err());
    }

    #[test]
    fn manual_completion_uses_milliseconds_against_git_evidence() {
        let manual = manually_completed_execution(&running(WorkflowRole::Execution), 1_420_000);
        let older_prepare = evidence(WorkflowRole::Prepare, git_timestamp_millis(1_400).unwrap());
        let newer_prepare = evidence(WorkflowRole::Prepare, git_timestamp_millis(1_435).unwrap());
        assert_eq!(
            newer_completed_state(&manual, manual.manual_completed_at.unwrap(), older_prepare)
                .current_role,
            WorkflowRole::Execution
        );
        assert_eq!(
            newer_completed_state(&manual, manual.manual_completed_at.unwrap(), newer_prepare)
                .current_role,
            WorkflowRole::Prepare
        );
    }

    #[test]
    fn automatic_execution_completion_has_no_manual_timestamp() {
        assert_eq!(
            completed_execution(&running(WorkflowRole::Execution), "E1".to_string())
                .manual_completed_at,
            None
        );
    }

    #[test]
    fn legacy_checkpoint_without_artifact_commit_deserializes() {
        let checkpoint: ProjectCheckpoint =
            serde_json::from_str(r#"{"currentRole":"qa","status":"completed"}"#).unwrap();
        assert_eq!(checkpoint.last_artifact_commit, None);
        assert_eq!(checkpoint.status, TrackerStatus::Completed);
    }
}
