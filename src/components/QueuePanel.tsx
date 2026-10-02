import { useEffect, useState } from "react";
import { advanceProjectWorkflow, completeExecutionWorkflow, getWorkflowHandoffTarget } from "../lib/tauriApi";
import { type ProjectId, type ProjectState, type QueueProjection, type WorkflowRole, stateForProject } from "../lib/queueContract";

const PROJECTS: Array<{ id: ProjectId; label: string }> = [
  { id: "noctua", label: "NOCTUA" },
  { id: "fgo", label: "FGO" },
];

export function QueuePanel({ projection }: { projection: QueueProjection }) {
  const [targets, setTargets] = useState<Partial<Record<ProjectId, WorkflowRole>>>({});

  useEffect(() => {
    let cancelled = false;
    void Promise.all(PROJECTS.map(async ({ id }) => [id, await getWorkflowHandoffTarget(id)] as const)).then((entries) => {
      if (!cancelled) {
        setTargets(Object.fromEntries(entries.filter(([, target]) => target)) as Partial<Record<ProjectId, WorkflowRole>>);
      }
    }).catch(() => undefined);
    return () => { cancelled = true; };
  }, [projection]);

  return (
    <section className="queue-panel" aria-label="개발 작업 현황">
      {PROJECTS.map((project) => (
        <QueueRow key={project.id} name={project.label} project={project.id} state={stateForProject(projection, project.id)} nextRole={targets[project.id] ?? null} />
      ))}
    </section>
  );
}

function QueueRow({ name, project, state, nextRole }: { name: string; project: ProjectId; state: ProjectState | null; nextRole: WorkflowRole | null }) {
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
      {state?.status === "completed" && nextRole ? (
        <button className="queue-handoff" type="button" onPointerDown={(event) => event.stopPropagation()} onClick={(event) => {
          event.stopPropagation();
          void advanceProjectWorkflow(project).catch(() => undefined);
        }} aria-label={`${name} 작업을 ${nextRole} 역할로 전달`} title={`${nextRole.toUpperCase()}로 전달`}>
          ▶ {nextRole.toUpperCase()}
        </button>
      ) : null}
      {state?.status === "running" && state.role === "execution" ? (
        <button className="queue-handoff" type="button" onPointerDown={(event) => event.stopPropagation()} onClick={(event) => {
          event.stopPropagation();
          void completeExecutionWorkflow(project).catch(() => undefined);
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
