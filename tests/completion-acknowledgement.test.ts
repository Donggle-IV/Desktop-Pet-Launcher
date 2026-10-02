import assert from "node:assert/strict";
import test from "node:test";
import {
  completionAcknowledgementForFocus,
  shouldConsumePendingAcknowledgement,
  shouldClearOneCycleAcknowledgement,
} from "../src/lib/completionAcknowledgement.ts";

test("focused completion is one-cycle while unfocused completion latches until focus", () => {
  assert.deepEqual(completionAcknowledgementForFocus(1, true), { mode: "one-cycle", token: 1 });
  assert.deepEqual(completionAcknowledgementForFocus(2, false), { mode: "pending-focus", token: 2 });
});

test("a stale one-cycle timer cannot clear a newer pending-focus acknowledgement", () => {
  const pending = completionAcknowledgementForFocus(2, false);
  assert.equal(shouldClearOneCycleAcknowledgement(pending, 1), false);
  assert.equal(shouldClearOneCycleAcknowledgement(pending, 2), false);
  assert.equal(shouldClearOneCycleAcknowledgement(completionAcknowledgementForFocus(1, true), 1), true);
});

test("one focus consumes all currently pending completion attention", () => {
  const pending = completionAcknowledgementForFocus(4, false);
  assert.equal(shouldConsumePendingAcknowledgement(pending, false), false);
  assert.equal(shouldConsumePendingAcknowledgement(pending, true), true);
  assert.equal(shouldConsumePendingAcknowledgement(completionAcknowledgementForFocus(4, true), true), false);
});
