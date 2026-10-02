import { useEffect, useRef, useState } from "react";
import { STATE_DEFINITIONS, type PetState } from "./petContract";
import {
  getAnimationCycleDuration as getCycleDuration,
  getAnimationFrameDuration,
} from "./animationTiming";

export function getAnimationCycleDuration(state: PetState, speed: number): number {
  return getCycleDuration(STATE_DEFINITIONS[state].durations, speed);
}

export function usePetAnimation(
  state: PetState,
  speed: number,
  reducedMotion: boolean,
): number {
  const [frame, setFrame] = useState(0);
  const timerRef = useRef<number | null>(null);

  useEffect(() => {
    if (timerRef.current !== null) {
      window.clearTimeout(timerRef.current);
      timerRef.current = null;
    }

    setFrame(0);
    if (reducedMotion) {
      return;
    }

    const definition = STATE_DEFINITIONS[state];
    let current = 0;
    const tick = () => {
      current = (current + 1) % definition.frames;
      setFrame(current);
      const duration = definition.durations[current] ?? 140;
      timerRef.current = window.setTimeout(tick, getAnimationFrameDuration(duration, speed));
    };

    timerRef.current = window.setTimeout(
      tick,
      getAnimationFrameDuration(definition.durations[0] ?? 140, speed),
    );

    return () => {
      if (timerRef.current !== null) {
        window.clearTimeout(timerRef.current);
      }
    };
  }, [reducedMotion, speed, state]);

  return frame;
}
