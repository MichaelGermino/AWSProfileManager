/**
 * Credential state, shown the same way everywhere: a status dot plus a mono countdown.
 * This is the app's one signature detail (see docs/native-rewrite-plan.md, Phase 2), so keep the
 * wording and thresholds here rather than repeating them per screen. The GPUI port mirrors it.
 */
import type { DashboardProfileSummary } from '../../shared/types';

/** Under this many seconds left, an active credential reads as "expiring" (amber). */
export const EXPIRING_SOON_SECONDS = 15 * 60;

export type CredentialState = 'active' | 'expiring' | 'expired' | 'never';

export function credentialState(
  status: DashboardProfileSummary['status'],
  timeRemainingSeconds: number | undefined
): CredentialState {
  if (status === 'expired') return 'expired';
  if (status === 'never') return 'never';
  if (timeRemainingSeconds !== undefined && timeRemainingSeconds < EXPIRING_SOON_SECONDS) return 'expiring';
  return 'active';
}

/** "2d 3h", "4h 05m", "47m", "<1m". Days appear past 48 hours so long sessions stay readable. */
export function formatCountdown(seconds: number): string {
  if (seconds < 60) return '<1m';
  const d = Math.floor(seconds / 86400);
  const h = Math.floor((seconds % 86400) / 3600);
  const m = Math.floor((seconds % 3600) / 60);
  const totalHours = Math.floor(seconds / 3600);
  if (totalHours >= 48) return `${d}d ${h}h`;
  if (totalHours > 0) return `${totalHours}h ${String(m).padStart(2, '0')}m`;
  return `${m}m`;
}

/** Main sends "12/31/2098, 4:00:00 PM" (Pacific). Drop the seconds and say which zone it is. */
export function formatExpiry(expiresAtPst: string): string {
  return `${expiresAtPst.replace(/:\d{2}(\s*[AP]M)$/i, '$1')} PT`;
}

/** Short text for the state: the countdown while active, otherwise a word. */
export function credentialLabel(state: CredentialState, timeRemainingSeconds: number | undefined): string {
  if (state === 'expired') return 'Expired';
  if (state === 'never') return 'Not signed in';
  return timeRemainingSeconds === undefined ? 'Active' : `${formatCountdown(timeRemainingSeconds)} left`;
}

const STATE_NAME: Record<CredentialState, string> = {
  active: 'Active',
  expiring: 'Expiring soon',
  expired: 'Expired',
  never: 'Not signed in',
};

interface CredentialStatusProps {
  status: DashboardProfileSummary['status'];
  timeRemainingSeconds?: number;
  expiresAtPst?: string;
  /** Show the absolute expiry time after the countdown. */
  showExpiry?: boolean;
  className?: string;
}

/** Dot + countdown. The dot's state is also given in words for screen readers. */
export function CredentialStatus({
  status,
  timeRemainingSeconds,
  expiresAtPst,
  showExpiry = false,
  className = '',
}: CredentialStatusProps) {
  const state = credentialState(status, timeRemainingSeconds);
  const label = credentialLabel(state, timeRemainingSeconds);
  const tone = state === 'expired' ? 'text-ui-danger' : state === 'expiring' ? 'text-ui-warning' : 'text-ui-textMuted';
  const line = (
    <span className="inline-flex min-w-0 items-center gap-2">
      <span className="status-dot" data-state={state} role="img" aria-label={STATE_NAME[state]} />
      <span className={`countdown whitespace-nowrap ${tone}`}>{label}</span>
    </span>
  );
  if (!showExpiry || !expiresAtPst) return <span className={`inline-flex min-w-0 ${className}`}>{line}</span>;
  // Two lines, so it sits level with the name / account pair beside it.
  return (
    <span className={`flex min-w-0 flex-col ${className}`}>
      {line}
      <span className="mono-meta truncate pl-4">
        {state === 'expired' ? 'at' : 'until'} {formatExpiry(expiresAtPst)}
      </span>
    </span>
  );
}
