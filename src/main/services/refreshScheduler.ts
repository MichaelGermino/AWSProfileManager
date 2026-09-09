import { getProfiles } from './profileStorage';
import { refreshProfile } from './awsAuthService';
import { isIdentityCenterProfile, orgOfProfile, orgKey } from '../../shared/ssoOrg';
import { shouldSkipDueToRecentNetworkFailure } from './networkStatus';
import { resetConsecutiveRefreshFailures } from './refreshFailureCounters';
import { isLocked } from './credentialStorage';
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
  // A locked app holds no master password, so neither a SAML IdP password nor an Identity Center
  // session can be decrypted. refreshProfile() refuses on its own, but bailing here as well keeps
  // the tick from marking every profile as freshly refreshed in lastScheduledRefreshAt — which
  // would push each one a full interval into the future for work that never happened.
  //
  // No wiring needed to resume: the next tick after unlocking simply proceeds.
  if (isLocked()) return;
  // Skip when we just saw a network-layer failure (offline/DNS). Avoids a burst of futile
  // attempts on suspend/resume; next tick will retry once the cooldown elapses.
  if (shouldSkipDueToRecentNetworkFailure()) return;
  const now = Date.now();
  const profiles = getProfiles().filter((p) => p.autoRefresh);
  /** Orgs already known to need an interactive sign-in this tick. */
  const blockedOrgs = new Set<string>();

  for (const profile of profiles) {
    if (isIdentityCenterProfile(profile)) {
      const org = orgOfProfile(profile);
      if (org && blockedOrgs.has(orgKey(org))) continue;
    }
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
      const result = await refreshProfile(profile.id);
      lastScheduledRefreshAt.set(profile.id, now);

      // An Identity Center profile whose SSO session needs a human is not a failure, and the
      // scheduler must never open a browser. Stop touching the rest of that org this tick:
      // one prompt is enough, and the remaining profiles would each re-emit the same event.
      if ('ssoLoginRequired' in result) {
        const org = orgOfProfile(profile);
        if (org) blockedOrgs.add(orgKey(org));
      }
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
