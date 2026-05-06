/**
 * Tracks the most recent network-layer failure across auth/refresh attempts.
 *
 * The scheduler uses this to skip ticks shortly after a confirmed offline error,
 * so suspending/resuming the laptop or briefly losing Wi-Fi doesn't fire a burst
 * of pointless requests (and doesn't risk hitting any failure thresholds).
 */

const SKIP_WINDOW_MS = 30_000;

let lastNetworkFailureAt = 0;

export function noteNetworkFailure(): void {
  lastNetworkFailureAt = Date.now();
}

export function clearNetworkFailure(): void {
  lastNetworkFailureAt = 0;
}

export function shouldSkipDueToRecentNetworkFailure(): boolean {
  if (lastNetworkFailureAt === 0) return false;
  return Date.now() - lastNetworkFailureAt < SKIP_WINDOW_MS;
}
