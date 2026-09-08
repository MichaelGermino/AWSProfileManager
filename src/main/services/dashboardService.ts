import { getProfiles } from './profileStorage';
import { readCredentialsFile } from './credentialsFile';
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

  /**
   * Status is cross-checked against the credentials file, not derived from profile.expiration
   * alone. `expiration` records when this profile last minted credentials, which says nothing
   * about whether they still exist on THIS machine — after a config restore (or a hand-edited
   * credentials file) a profile would otherwise show a live countdown while `--profile x` fails.
   */
  // null means "couldn't read it", which is different from "it's empty". A missing or empty
  // credentials file genuinely means no profile has credentials — the exact fresh-machine
  // restore case this check exists for — whereas a malformed one tells us nothing, so we fall
  // back to expiration-only rather than reporting every profile as never-refreshed.
  let sections: Record<string, unknown> | null = null;
  try {
    sections = readCredentialsFile();
  } catch {
    sections = null;
  }

  return profiles.map((p) => {
    const sectionName = (p.credentialProfileName || p.name || '').trim();
    const sectionMissing = sections !== null && !!sectionName && !(sectionName in sections);

    const exp = p.expiration ? new Date(p.expiration).getTime() : 0;
    const isExpired = exp > 0 && exp <= now;
    const never = !p.expiration || sectionMissing;
    const status: DashboardProfileSummary['status'] = never ? 'never' : isExpired ? 'expired' : 'active';
    const timeRemainingSeconds =
      p.expiration && !sectionMissing ? getTimeRemainingSeconds(p.expiration) : undefined;
    return {
      id: p.id,
      name: p.name,
      accountNumber: p.roleDisplayText ?? p.accountNumber ?? p.label ?? '',
      label: p.label,
      status,
      // Suppressed when the section is gone: an expiry for credentials that no longer exist
      // reads as reassurance the user shouldn't have.
      expiresAtPst: sectionMissing ? undefined : formatPst(p.expiration),
      timeRemainingSeconds: status === 'active' ? timeRemainingSeconds : undefined,
      iconName: p.iconName,
      iconColor: p.iconColor,
    };
  });
}
