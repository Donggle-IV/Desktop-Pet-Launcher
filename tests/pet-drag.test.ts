import assert from "node:assert/strict";
import test from "node:test";
import { applyPetDragMovement, createPetDragState } from "../src/lib/petDrag.ts";

test("drag direction changes are observable before the caller records them", () => {
  const drag = createPetDragState({
    pointerId: 1,
    originX: 10,
    originY: 20,
    windowOffsetX: 0,
    windowOffsetY: 0,
  });

  const first = applyPetDragMovement(drag, 4, 0);
  assert.equal(first?.direction, "running-right");
  assert.equal(drag.lastDirection, null);

  drag.lastDirection = first?.direction ?? null;
  const second = applyPetDragMovement(drag, -10, 0);
  assert.equal(second?.direction, "running-left");
  assert.notEqual(second?.direction, drag.lastDirection);
});
