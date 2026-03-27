import { getProfiles } from './profileStorage';
import { refreshProfile } from './awsAuthService';
import { resetConsecutiveRefreshFailures } from './refreshFailureCounters';
import { sendToRenderer } from './ipcBridge';
import {
  getRefreshPausedDueToFailuresPref,
  getRefreshPausedPref,
  setRefreshPausedDueToFailuresPref,
  setRefreshPausedPref,
} from './uiPrefsService';

const CHECK_INTERVAL_MS = 60 * 1000; // run the check every minute
const REFRESH_THRESHOLD_MINUTES = 15; // also refresh when cred expires within this many minutes

export type RefreshPauseState = { paused: boolean; pausedDueToFailures: boolean };

// Load persisted value at module load so getRefreshPauseState() is correct before startScheduler() runs
let paused = (() => {
  try {
    return getRefreshPausedPref();
  } catch {
    return false;
  }
})();
let pausedDueToFailures = (() => {
  try {
    return getRefreshPausedDueToFailuresPref();
  } catch {
    return false;
  }
})();
let intervalId: ReturnType<typeof setInterval> | null = null;
/** Last time we ran a scheduled (auto) refresh per profile. Interval is "time since last scheduled refresh"
 * for that profile, not time until credential expiry. So with a 1h interval, we refresh every hour even
 * if the current creds still have 48 minutes left. */
const lastScheduledRefreshAt = new Map<string, number>();

export function getRefreshPaused(): boolean {
  return paused;
}

export function getRefreshPauseState(): RefreshPauseState {
  return { paused, pausedDueToFailures };
}

export function setRefreshPaused(value: boolean, options?: { dueToFailures?: boolean }): void {
  paused = value;
  setRefreshPausedPref(value);
  if (value) {
    if (options?.dueToFailures) {
      pausedDueToFailures = true;
      setRefreshPausedDueToFailuresPref(true);
    } else {
      pausedDueToFailures = false;
      setRefreshPausedDueToFailuresPref(false);
    }
  } else {
    resetConsecutiveRefreshFailures();
    pausedDueToFailures = false;
    setRefreshPausedDueToFailuresPref(false);
  }
  sendToRenderer('scheduler:pausedChanged', { paused: value, pausedDueToFailures });
}

function shouldRefreshByExpiration(expiration: string | undefined): boolean {
  if (!expiration) return true;
  const exp = new Date(expiration).getTime();
  const threshold = Date.now() + REFRESH_THRESHOLD_MINUTES * 60 * 1000;
  return exp <= threshold;
}

async function runScheduledRefresh(): Promise<void> {
  if (paused) return;
  const now = Date.now();
  const profiles = getProfiles().filter((p) => p.autoRefresh);
  for (const profile of profiles) {
    const intervalMs = profile.refreshIntervalMinutes * 60 * 1000;
    const lastAt = lastScheduledRefreshAt.get(profile.id) ?? 0;
    const hasLastRefresh = lastAt > 0;
    const intervalElapsed = hasLastRefresh && now - lastAt >= intervalMs;
    const expiringSoon = shouldRefreshByExpiration(profile.expiration);
    if (!intervalElapsed && !expiringSoon) {
      if (!hasLastRefresh) lastScheduledRefreshAt.set(profile.id, now);
      continue;
    }
    try {
      await refreshProfile(profile.id);
      lastScheduledRefreshAt.set(profile.id, now);
    } catch {
      // per-profile errors are handled in refreshProfile
    }
  }
}

export function startScheduler(): void {
  if (intervalId) return;
  intervalId = setInterval(runScheduledRefresh, CHECK_INTERVAL_MS);
}

export function stopScheduler(): void {
  if (intervalId) {
    clearInterval(intervalId);
    intervalId = null;
  }
}
