import { useCallback, useEffect, useState } from "react";
import { advanceProjectWorkflow, completeExecutionWorkflow, getProjectWorkflow, type TrackerProjectView } from "../lib/tauriApi";
import { type ProjectId, type ProjectState, type QueueProjection, type WorkflowRole, stateForProject } from "../lib/queueContract";

const PROJECTS: Array<{ id: ProjectId; label: string }> = [
  { id: "noctua", label: "NOCTUA" },
  { id: "fgo", label: "FGO" },
];

export function QueuePanel({ projection }: { projection: QueueProjection }) {
  const [workflows, setWorkflows] = useState<Partial<Record<ProjectId, TrackerProjectView>>>({});

  const refreshWorkflows = useCallback(() => {
    void Promise.all(PROJECTS.map(async ({ id }) => [id, await getProjectWorkflow(id)] as const)).then((entries) => {
      setWorkflows(Object.fromEntries(entries.filter(([, workflow]) => workflow)) as Partial<Record<ProjectId, TrackerProjectView>>);
    }).catch(() => undefined);
  }, []);
  useEffect(() => {
    refreshWorkflows();
  }, [projection, refreshWorkflows]);

  return (
    <section className="queue-panel" aria-label="개발 작업 현황">
      {PROJECTS.map((project) => (
        <QueueRow key={project.id} name={project.label} project={project.id} state={stateForProject(projection, project.id)} workflow={workflows[project.id] ?? null} refreshWorkflows={refreshWorkflows} />
      ))}
    </section>
  );
}

function QueueRow({ name, project, state, workflow, refreshWorkflows }: { name: string; project: ProjectId; state: ProjectState | null; workflow: TrackerProjectView | null; refreshWorkflows: () => void }) {
  const nextRole = workflow?.nextRole ?? null;
  const display = state ? formatState(state) : { status: "UNKNOWN", role: null, label: null, tone: "unknown" };
  return (
    <div className={`queue-row is-${display.tone}`}>
      <strong>{name}</strong>
      <span className="queue-summary">
        {display.role ? <span className="queue-role">{display.role}</span> : null}
        <span className="queue-status">{display.status}</span>
      </span>
      {display.label ? (
        <span className="queue-label" title={display.label}>
          {display.label}
        </span>
      ) : null}
      {state?.status === "completed" && workflow && nextRole ? (
        <button className="queue-handoff" type="button" onPointerDown={(event) => event.stopPropagation()} onClick={(event) => {
          event.stopPropagation();
          void advanceProjectWorkflow(project, workflow.trackerRevision, nextRole).then((result) => {
            if (result !== "advanced") refreshWorkflows();
          }).catch(() => refreshWorkflows());
        }} aria-label={`${name} 작업을 ${nextRole} 역할로 전달`} title={`${nextRole.toUpperCase()}로 전달`}>
          ▶ {nextRole.toUpperCase()}
        </button>
      ) : null}
      {state?.status === "running" && state.role === "execution" ? (
        <button className="queue-handoff" type="button" onPointerDown={(event) => event.stopPropagation()} onClick={(event) => {
          event.stopPropagation();
          if (!workflow) return;
          void completeExecutionWorkflow(project, workflow.trackerRevision).then((result) => {
            if (result !== "advanced") refreshWorkflows();
          }).catch(() => refreshWorkflows());
        }} aria-label={`${name} Execution 작업 완료`} title="Execution 완료">
          ✓ COMPLETE
        </button>
      ) : null}
    </div>
  );
}

function formatState(state: ProjectState): {
  status: string;
  role: string | null;
  label: string | null;
  tone: string;
} {
  if (state.status === "idle") {
    return { status: "IDLE", role: null, label: null, tone: "idle" };
  }
  return {
    status: state.status.toUpperCase(),
    role: state.role.toUpperCase(),
    label: state.label,
    tone: state.status,
  };
}
