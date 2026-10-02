export function getAnimationFrameDuration(duration: number, speed: number): number {
  const safeSpeed = Number.isFinite(speed) && speed > 0 ? speed : 1;
  return Math.max(40, duration / safeSpeed);
}

export function getAnimationCycleDuration(durations: readonly number[], speed: number): number {
  return durations.reduce((total, duration) => total + getAnimationFrameDuration(duration, speed), 0);
}
