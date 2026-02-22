import { getProfiles } from './profileStorage';
import type { DashboardProfileSummary } from '../../shared/types';

function formatPst(isoString: string | undefined): string | undefined {
  if (!isoString) return undefined;
  try {
    const date = new Date(isoString);
    return date.toLocaleString('en-US', { timeZone: 'America/Los_Angeles' });
  } catch {
    return isoString;
  }
}

function getTimeRemainingSeconds(expiration: string | undefined): number | undefined {
  if (!expiration) return undefined;
  const exp = new Date(expiration).getTime();
  const now = Date.now();
  const diff = Math.floor((exp - now) / 1000);
  return diff > 0 ? diff : 0;
}

export function getDashboardState(): DashboardProfileSummary[] {
  const profiles = getProfiles();
  const now = Date.now();
  return profiles.map((p) => {
    const exp = p.expiration ? new Date(p.expiration).getTime() : 0;
    const isExpired = exp > 0 && exp <= now;
    const never = !p.expiration;
    const status: DashboardProfileSummary['status'] = never ? 'never' : isExpired ? 'expired' : 'active';
    const timeRemainingSeconds = p.expiration ? getTimeRemainingSeconds(p.expiration) : undefined;
    return {
      id: p.id,
      name: p.name,
      accountNumber: p.roleDisplayText ?? p.accountNumber ?? p.label ?? '',
      label: p.label,
      status,
      expiresAtPst: formatPst(p.expiration),
      timeRemainingSeconds: status === 'active' ? timeRemainingSeconds : undefined,
      iconName: p.iconName,
      iconColor: p.iconColor,
    };
  });
}
