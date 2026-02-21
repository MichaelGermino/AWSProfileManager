import { getProfiles } from './profileStorage';
import { refreshProfile } from './awsAuthService';

const CHECK_INTERVAL_MS = 60 * 1000; // every minute
const REFRESH_THRESHOLD_MINUTES = 15;

let paused = false;
let intervalId: ReturnType<typeof setInterval> | null = null;

export function getRefreshPaused(): boolean {
  return paused;
}

export function setRefreshPaused(value: boolean): void {
  paused = value;
}

function shouldRefresh(expiration: string | undefined): boolean {
  if (!expiration) return true;
  const exp = new Date(expiration).getTime();
  const threshold = Date.now() + REFRESH_THRESHOLD_MINUTES * 60 * 1000;
  return exp <= threshold;
}

async function runScheduledRefresh(): Promise<void> {
  if (paused) return;
  const profiles = getProfiles().filter((p) => p.autoRefresh);
  for (const profile of profiles) {
    if (!shouldRefresh(profile.expiration)) continue;
    try {
      await refreshProfile(profile.id);
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
