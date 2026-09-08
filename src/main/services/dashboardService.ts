import { getProfiles } from './profileStorage';
import { readCredentialsFile } from './credentialsFile';
import { getSettings } from './settingsService';
import type { DashboardProfileSummary, Profile } from '../../shared/types';

/** Account id + role for a profile, from the SSO fields or parsed out of the SAML role ARN. */
function accountAndRole(p: Profile): { accountId?: string; roleName?: string } {
  if (p.ssoAccountId) return { accountId: p.ssoAccountId, roleName: p.ssoRoleName };
  const m = p.roleArn?.match(/arn:aws:iam::(\d+):role\/(.+)/);
  return m ? { accountId: m[1], roleName: m[2] } : {};
}

/**
 * Compact "account - role" label for menus, resolved at read time from accountDisplayNames.
 *
 * Deliberately omits the account number that the profile list shows: a tray submenu is narrow, and
 * the friendly name plus role is what identifies a profile at a glance. Falls back through the
 * account id to the profile name so it can never render blank.
 */
export function profileMenuLabel(p: Profile, displayNames: Record<string, string>): string {
  const { accountId, roleName } = accountAndRole(p);
  const account = (accountId ? displayNames[accountId]?.trim() : undefined) ?? accountId;
  if (account && roleName) return `${account} - ${roleName}`;
  return p.roleDisplayText?.trim() || p.name;
}

/**
 * Label for the profile list, resolved at read time.
 *
 * profile.roleDisplayText is a snapshot taken when the account/role was chosen, so a display name
 * added or edited in Settings afterwards would never show up — the stored text kept the raw AWS
 * account name forever, and re-saving the profile didn't help because only the dropdown's
 * onChange recomputes it. Looking the name up here means Settings edits apply immediately, to
 * both SSO and SAML profiles, with no migration of stored data.
 */
function resolveDisplayText(p: Profile, displayNames: Record<string, string>): string {
  const { accountId, roleName } = accountAndRole(p);
  const friendly = accountId ? displayNames[accountId]?.trim() : undefined;
  if (friendly && accountId) {
    return roleName ? `${friendly} (${accountId}) - ${roleName}` : `${friendly} (${accountId})`;
  }
  return p.roleDisplayText ?? p.accountNumber ?? p.label ?? '';
}

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

  let displayNames: Record<string, string> = {};
  try {
    displayNames = getSettings().accountDisplayNames ?? {};
  } catch {
    displayNames = {};
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
      accountNumber: resolveDisplayText(p, displayNames),
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
