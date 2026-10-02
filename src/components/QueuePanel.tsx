import { type ProjectId, type ProjectState, type QueueProjection, stateForProject } from "../lib/queueContract";

const PROJECTS: Array<{ id: ProjectId; label: string }> = [
  { id: "noctua", label: "NOCTUA" },
  { id: "fgo", label: "FGO" },
];

export function QueuePanel({ projection }: { projection: QueueProjection }) {
  return (
    <section className="queue-panel" aria-label="개발 작업 현황">
      {PROJECTS.map((project) => (
        <QueueRow key={project.id} name={project.label} state={stateForProject(projection, project.id)} />
      ))}
    </section>
  );
}

function QueueRow({ name, state }: { name: string; state: ProjectState | null }) {
  const display = state ? formatState(state) : { status: "UNKNOWN", role: "", label: "—", tone: "unknown" };
  return (
    <div className={`queue-row is-${display.tone}`}>
      <strong>{name}</strong>
      <span className="queue-status">{display.status}</span>
      <span className="queue-role">{display.role}</span>
      <span className="queue-label" title={display.label}>
        {display.label}
      </span>
    </div>
  );
}

function formatState(state: ProjectState): {
  status: string;
  role: string;
  label: string;
  tone: string;
} {
  if (state.status === "idle") {
    return { status: "IDLE", role: "", label: "○", tone: "idle" };
  }
  return {
    status: state.status.toUpperCase(),
    role: state.role.toUpperCase(),
    label: state.label ?? "—",
    tone: state.status,
  };
}
