import assert from "node:assert/strict";
import test from "node:test";
import { deriveQueueVisualState, resolveVisualState } from "../src/lib/resolveVisualState.ts";
import type { ProjectState, QueueProjection } from "../src/lib/queueContract.ts";

function active(status: Exclude<ProjectState["status"], "idle">, role: "prepare" | "qa" | "execution"): ProjectState {
  return { status, role, label: null, attentionRequired: false, updatedAt: 1 };
}

function queue(noctua: ProjectState | null, fgo: ProjectState | null): QueueProjection {
  return { revision: 1, noctua, fgo };
}

test("queue summary arbitration uses the documented priority", () => {
  assert.equal(deriveQueueVisualState(queue(active("failed", "qa"), active("running", "execution"))), "failed");
  assert.equal(deriveQueueVisualState(queue(active("blocked", "qa"), active("running", "execution"))), "failed");
  assert.equal(deriveQueueVisualState(queue(active("waiting", "qa"), active("running", "execution"))), "waiting");
  assert.equal(deriveQueueVisualState(queue(active("running", "prepare"), active("running", "qa"))), "review");
  assert.equal(deriveQueueVisualState(queue(active("completed", "qa"), active("running", "execution"))), "running");
  assert.equal(deriveQueueVisualState(queue(active("completed", "qa"), active("completed", "execution"))), null);
  assert.equal(deriveQueueVisualState(queue({ status: "idle", role: null, label: null, attentionRequired: false, updatedAt: 1 }, null)), null);
});

test("physical, chat, manual, and acknowledgement overrides precede queue state", () => {
  const common = {
    queue: queue(active("failed", "execution"), null),
    idleVariant: "review" as const,
  };
  assert.equal(resolveVisualState({ ...common, dragState: "running-left", conversationState: "running", manualState: "review", completionAcknowledgement: true }), "running-left");
  assert.equal(resolveVisualState({ ...common, dragState: null, conversationState: "running", manualState: "review", completionAcknowledgement: true }), "running");
  assert.equal(resolveVisualState({ ...common, dragState: null, conversationState: null, manualState: "review", completionAcknowledgement: true }), "review");
  assert.equal(resolveVisualState({ ...common, dragState: null, conversationState: null, manualState: "idle", completionAcknowledgement: true }), "waving");
});
