export const PROJECT_IDS = ["noctua", "fgo"] as const;
export type ProjectId = (typeof PROJECT_IDS)[number];

export type WorkflowRole = "prepare" | "qa" | "execution";
export type ActiveWorkflowStatus = "running" | "waiting" | "blocked" | "failed" | "completed";

export interface IdleProjectState {
  status: "idle";
  role: null;
  label: null;
  attentionRequired: false;
  updatedAt: number;
}

export interface ActiveProjectState {
  status: ActiveWorkflowStatus;
  role: WorkflowRole;
  label: string | null;
  attentionRequired: boolean;
  updatedAt: number;
}

export type ProjectState = IdleProjectState | ActiveProjectState;

export interface QueueProjection {
  revision: number;
  noctua: ProjectState | null;
  fgo: ProjectState | null;
}

export interface QueueProjectCompletedEvent {
  project: ProjectId;
  role: WorkflowRole;
  revision: number;
}

export const EMPTY_QUEUE_PROJECTION: QueueProjection = {
  revision: 0,
  noctua: null,
  fgo: null,
};

export function stateForProject(projection: QueueProjection, project: ProjectId): ProjectState | null {
  return projection[project];
}
