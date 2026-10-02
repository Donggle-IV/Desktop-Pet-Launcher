import type { PetState } from "./petContract";
import type { ProjectState, QueueProjection } from "./queueContract";

export interface VisualStateInput {
  dragState: "running-left" | "running-right" | null;
  conversationState: PetState | null;
  manualState: PetState;
  completionAcknowledgement: boolean;
  queue: QueueProjection;
  idleVariant: PetState;
}

export function resolveVisualState(input: VisualStateInput): PetState {
  return (
    input.dragState ??
    input.conversationState ??
    (input.manualState === "idle" ? null : input.manualState) ??
    (input.completionAcknowledgement ? "waving" : null) ??
    deriveQueueVisualState(input.queue) ??
    input.idleVariant
  );
}

export function deriveQueueVisualState(queue: QueueProjection): PetState | null {
  const states = [queue.noctua, queue.fgo].filter((state): state is ProjectState => state !== null);
  if (states.some((state) => state.status === "failed" || state.status === "blocked")) {
    return "failed";
  }
  if (states.some((state) => state.status === "waiting" || state.attentionRequired)) {
    return "waiting";
  }
  if (states.some((state) => state.status === "running" && state.role === "execution")) {
    return "running";
  }
  if (
    states.some(
      (state) => state.status === "running" && (state.role === "prepare" || state.role === "qa"),
    )
  ) {
    return "review";
  }
  return null;
}
