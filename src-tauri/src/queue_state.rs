use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub(crate) enum ProjectId {
    Noctua,
    Fgo,
}

impl ProjectId {
    pub(crate) fn parse(value: &str) -> Option<Self> {
        match value {
            "noctua" => Some(Self::Noctua),
            "fgo" => Some(Self::Fgo),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub(crate) enum WorkflowRole {
    Prepare,
    Qa,
    Execution,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub(crate) enum ActiveWorkflowStatus {
    Running,
    Waiting,
    Blocked,
    Failed,
    Completed,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProjectStateInput {
    pub(crate) status: InputStatus,
    pub(crate) role: Option<WorkflowRole>,
    pub(crate) label: Option<String>,
    pub(crate) attention_required: Option<bool>,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub(crate) enum InputStatus {
    Idle,
    Running,
    Waiting,
    Blocked,
    Failed,
    Completed,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProjectSnapshot {
    pub(crate) status: SnapshotStatus,
    pub(crate) role: Option<WorkflowRole>,
    pub(crate) label: Option<String>,
    pub(crate) attention_required: bool,
    pub(crate) updated_at: u64,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub(crate) enum SnapshotStatus {
    Idle,
    Running,
    Waiting,
    Blocked,
    Failed,
    Completed,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct QueueProjection {
    pub(crate) revision: u64,
    pub(crate) noctua: Option<ProjectSnapshot>,
    pub(crate) fgo: Option<ProjectSnapshot>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProjectCompletedEvent {
    pub(crate) project: ProjectId,
    pub(crate) role: WorkflowRole,
    pub(crate) revision: u64,
}

#[derive(Debug, Clone)]
pub(crate) struct QueueMutation {
    pub(crate) projection: QueueProjection,
    pub(crate) completed: Option<ProjectCompletedEvent>,
}

#[derive(Debug, Clone)]
enum StoredProjectState {
    Idle {
        updated_at: u64,
    },
    Active {
        role: WorkflowRole,
        status: ActiveWorkflowStatus,
        label: Option<String>,
        attention_required: bool,
        updated_at: u64,
    },
}

impl StoredProjectState {
    fn status(&self) -> Option<ActiveWorkflowStatus> {
        match self {
            Self::Idle { .. } => None,
            Self::Active { status, .. } => Some(*status),
        }
    }

    fn snapshot(&self) -> ProjectSnapshot {
        match self {
            Self::Idle { updated_at } => ProjectSnapshot {
                status: SnapshotStatus::Idle,
                role: None,
                label: None,
                attention_required: false,
                updated_at: *updated_at,
            },
            Self::Active {
                role,
                status,
                label,
                attention_required,
                updated_at,
            } => ProjectSnapshot {
                status: match status {
                    ActiveWorkflowStatus::Running => SnapshotStatus::Running,
                    ActiveWorkflowStatus::Waiting => SnapshotStatus::Waiting,
                    ActiveWorkflowStatus::Blocked => SnapshotStatus::Blocked,
                    ActiveWorkflowStatus::Failed => SnapshotStatus::Failed,
                    ActiveWorkflowStatus::Completed => SnapshotStatus::Completed,
                },
                role: Some(*role),
                label: label.clone(),
                attention_required: *attention_required,
                updated_at: *updated_at,
            },
        }
    }
}

#[derive(Default)]
struct QueueRuntimeInner {
    revision: u64,
    noctua: Option<StoredProjectState>,
    fgo: Option<StoredProjectState>,
}

#[derive(Clone, Default)]
pub(crate) struct QueueRuntime {
    inner: Arc<Mutex<QueueRuntimeInner>>,
}

impl QueueRuntime {
    pub(crate) fn projection(&self) -> QueueProjection {
        let inner = self.inner.lock().expect("queue runtime lock poisoned");
        projection_from_inner(&inner)
    }

    pub(crate) fn replace(
        &self,
        project: ProjectId,
        input: ProjectStateInput,
    ) -> Result<QueueMutation, String> {
        let next = validate_input(input)?;
        let mut inner = self.inner.lock().expect("queue runtime lock poisoned");
        let previous = project_slot(&inner, project)
            .as_ref()
            .and_then(StoredProjectState::status);
        inner.revision = inner.revision.saturating_add(1);
        let revision = inner.revision;
        *project_slot_mut(&mut inner, project) = Some(next.clone());
        let completed = match next {
            StoredProjectState::Active {
                role,
                status: ActiveWorkflowStatus::Completed,
                ..
            } if previous != Some(ActiveWorkflowStatus::Completed) => Some(ProjectCompletedEvent {
                project,
                role,
                revision,
            }),
            _ => None,
        };
        Ok(QueueMutation {
            projection: projection_from_inner(&inner),
            completed,
        })
    }

    pub(crate) fn clear(&self, project: ProjectId) -> QueueMutation {
        let mut inner = self.inner.lock().expect("queue runtime lock poisoned");
        inner.revision = inner.revision.saturating_add(1);
        *project_slot_mut(&mut inner, project) = None;
        QueueMutation {
            projection: projection_from_inner(&inner),
            completed: None,
        }
    }
}

fn validate_input(input: ProjectStateInput) -> Result<StoredProjectState, String> {
    let label = normalize_label(input.label)?;
    let updated_at = unix_time_millis();
    match input.status {
        InputStatus::Idle => {
            if input.role.is_some() || label.is_some() || input.attention_required.is_some() {
                return Err(
                    "idle state must not include role, label, or attentionRequired".to_string(),
                );
            }
            Ok(StoredProjectState::Idle { updated_at })
        }
        status => {
            let role = input
                .role
                .ok_or_else(|| "active workflow states require role".to_string())?;
            let status = match status {
                InputStatus::Running => ActiveWorkflowStatus::Running,
                InputStatus::Waiting => ActiveWorkflowStatus::Waiting,
                InputStatus::Blocked => ActiveWorkflowStatus::Blocked,
                InputStatus::Failed => ActiveWorkflowStatus::Failed,
                InputStatus::Completed => ActiveWorkflowStatus::Completed,
                InputStatus::Idle => unreachable!(),
            };
            let attention_required = matches!(
                status,
                ActiveWorkflowStatus::Waiting | ActiveWorkflowStatus::Blocked
            ) || input.attention_required.unwrap_or(false);
            Ok(StoredProjectState::Active {
                role,
                status,
                label,
                attention_required,
                updated_at,
            })
        }
    }
}

fn normalize_label(label: Option<String>) -> Result<Option<String>, String> {
    let Some(label) = label else {
        return Ok(None);
    };
    let trimmed = label.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    if trimmed.chars().count() > 120 {
        return Err("label must be 120 characters or fewer".to_string());
    }
    Ok(Some(trimmed.to_string()))
}

fn unix_time_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

fn project_slot(inner: &QueueRuntimeInner, project: ProjectId) -> &Option<StoredProjectState> {
    match project {
        ProjectId::Noctua => &inner.noctua,
        ProjectId::Fgo => &inner.fgo,
    }
}

fn project_slot_mut(
    inner: &mut QueueRuntimeInner,
    project: ProjectId,
) -> &mut Option<StoredProjectState> {
    match project {
        ProjectId::Noctua => &mut inner.noctua,
        ProjectId::Fgo => &mut inner.fgo,
    }
}

fn projection_from_inner(inner: &QueueRuntimeInner) -> QueueProjection {
    QueueProjection {
        revision: inner.revision,
        noctua: inner.noctua.as_ref().map(StoredProjectState::snapshot),
        fgo: inner.fgo.as_ref().map(StoredProjectState::snapshot),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn active(status: InputStatus) -> ProjectStateInput {
        ProjectStateInput {
            status,
            role: Some(WorkflowRole::Execution),
            label: Some("TRANCHE-1".to_string()),
            attention_required: Some(false),
        }
    }

    #[test]
    fn startup_is_unknown_and_idle_is_explicit() {
        let runtime = QueueRuntime::default();
        assert!(runtime.projection().noctua.is_none());
        let mutation = runtime
            .replace(
                ProjectId::Noctua,
                ProjectStateInput {
                    status: InputStatus::Idle,
                    role: None,
                    label: None,
                    attention_required: None,
                },
            )
            .unwrap();
        assert_eq!(
            mutation.projection.noctua.unwrap().status,
            SnapshotStatus::Idle
        );
    }

    #[test]
    fn active_states_require_roles_and_attention_cannot_suppress_waiting() {
        let runtime = QueueRuntime::default();
        assert!(runtime
            .replace(
                ProjectId::Noctua,
                ProjectStateInput {
                    status: InputStatus::Running,
                    role: None,
                    label: None,
                    attention_required: None,
                },
            )
            .is_err());
        let waiting = runtime
            .replace(ProjectId::Noctua, active(InputStatus::Waiting))
            .unwrap();
        assert!(waiting.projection.noctua.unwrap().attention_required);
    }

    #[test]
    fn completion_is_emitted_once_per_transition_and_clear_returns_unknown() {
        let runtime = QueueRuntime::default();
        assert!(runtime
            .replace(ProjectId::Fgo, active(InputStatus::Completed))
            .unwrap()
            .completed
            .is_some());
        assert!(runtime
            .replace(ProjectId::Fgo, active(InputStatus::Completed))
            .unwrap()
            .completed
            .is_none());
        let cleared = runtime.clear(ProjectId::Fgo);
        assert!(cleared.projection.fgo.is_none());
        assert_eq!(cleared.projection.revision, 3);
    }
}
