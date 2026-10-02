import assert from "node:assert/strict";
import test from "node:test";
import { getAnimationCycleDuration } from "../src/lib/animationTiming.ts";

test("waving acknowledgement duration follows animation speed and the frame minimum", () => {
  const wavingDurations = [140, 140, 140, 280];
  assert.equal(getAnimationCycleDuration(wavingDurations, 1), 700);
  assert.equal(getAnimationCycleDuration(wavingDurations, 0.5), 1400);
  assert.ok(Math.abs(getAnimationCycleDuration(wavingDurations, 3) - 700 / 3) < 0.001);
  assert.equal(getAnimationCycleDuration(wavingDurations, 100), 160);
});
