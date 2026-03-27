const THRESHOLD = 2;

let consecutiveFailures = 0;

/** Call after a failed refresh attempt (IdP/STS error in refreshProfile catch). Returns true when threshold is reached (second consecutive failure). */
export function recordConsecutiveRefreshFailure(): boolean {
  consecutiveFailures++;
  return consecutiveFailures === THRESHOLD;
}

export function resetConsecutiveRefreshFailures(): void {
  consecutiveFailures = 0;
}
