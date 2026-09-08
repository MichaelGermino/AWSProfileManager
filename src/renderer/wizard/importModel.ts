import { v4 as uuidv4 } from 'uuid';
import type { AwsRole, Profile, SsoAccount } from '../../shared/types';

/**
 * One normalized shape for both importers.
 *
 * SAML and Identity Center discovery return different things — `AwsRole[]` with ARNs versus
 * `SsoAccount[]` with account/role names — but the user-facing job is identical: pick account/role
 * pairs and create a profile for each. Normalizing here means the picker UI and the profile-building
 * logic are written once.
 */

export interface ImportableRole {
  /** Unique within the import: the role ARN for SAML, `accountId|roleName` for Identity Center. */
  key: string;
  roleName: string;
  /** SAML only — the ARN pair needed to assume the role. */
  saml?: { roleArn: string; principalArn: string };
}

export interface ImportableAccount {
  accountId: string;
  accountName: string;
  email?: string;
  roles: ImportableRole[];
}

export type ImportSource =
  | { kind: 'identityCenter'; startUrl: string; region: string }
  | { kind: 'saml'; idpEntryUrl: string };

const ROLE_ARN = /arn:aws:iam::(\d+):role\/(.+)/;

/** Sort accounts by name and roles by name: both APIs return unstable order between calls. */
function sorted(accounts: ImportableAccount[]): ImportableAccount[] {
  for (const a of accounts) a.roles.sort((x, y) => x.roleName.localeCompare(y.roleName));
  return accounts.sort((a, b) => a.accountName.localeCompare(b.accountName));
}

export function fromSsoAccounts(
  accounts: SsoAccount[],
  displayNames: Record<string, string> = {}
): ImportableAccount[] {
  return sorted(
    accounts.map((a) => ({
      accountId: a.accountId,
      // A name the user configured in Settings wins over the raw AWS account name.
      accountName: displayNames[a.accountId]?.trim() || a.accountName || a.accountId,
      email: a.emailAddress,
      roles: a.roles.map((roleName) => ({ key: `${a.accountId}|${roleName}`, roleName })),
    }))
  );
}

/**
 * SAML roles arrive as a flat list of ARNs; group them by account.
 *
 * `AwsRole.accountName` is only populated when the IdP page exposes it — many ADFS pages do not,
 * which is why an import could show nothing but account numbers. Fall back to the name the user
 * configured in Settings before giving up and showing the bare account id.
 *
 * Roles whose ARN doesn't parse are dropped — they cannot produce a usable profile anyway.
 */
export function fromSamlRoles(
  roles: AwsRole[],
  displayNames: Record<string, string> = {}
): ImportableAccount[] {
  const byAccount = new Map<string, ImportableAccount>();

  for (const role of roles) {
    const match = role.roleArn.match(ROLE_ARN);
    if (!match) continue;
    const [, accountId, roleName] = match;

    let account = byAccount.get(accountId);
    if (!account) {
      const name = role.accountName?.trim() || displayNames[accountId]?.trim() || accountId;
      account = { accountId, accountName: name, roles: [] };
      byAccount.set(accountId, account);
    } else if (!account.accountName || account.accountName === accountId) {
      // A later role for the same account may be the one carrying the friendly name.
      if (role.accountName?.trim()) account.accountName = role.accountName.trim();
    }

    account.roles.push({
      key: role.roleArn,
      roleName,
      saml: { roleArn: role.roleArn, principalArn: role.principalArn },
    });
  }

  return sorted([...byAccount.values()]);
}

/** Credentials-file section names can't contain whitespace or brackets without confusing consumers. */
export function toCredentialSectionName(raw: string): string {
  return raw
    .trim()
    .replace(/[^A-Za-z0-9._-]+/g, '-')
    .replace(/-+/g, '-')
    .replace(/^-|-$/g, '')
    .toLowerCase();
}

function displayTextFor(
  account: ImportableAccount,
  roleName: string,
  displayNames: Record<string, string>
): string {
  const friendly = displayNames[account.accountId]?.trim() || account.accountName;
  return friendly && friendly !== account.accountId
    ? `${friendly} (${account.accountId}) - ${roleName}`
    : `${account.accountId} / ${roleName}`;
}

export interface BuildProfilesInput {
  accounts: ImportableAccount[];
  /** Role keys the user checked. */
  selected: Set<string>;
  source: ImportSource;
  /** Existing profiles, so generated section names don't collide with them. */
  existingProfiles: Profile[];
  existingDisplayNames: Record<string, string>;
  refreshIntervalMinutes: number;
  /** SAML only: whether default IdP credentials are actually stored. */
  hasStoredCredentials?: boolean;
}

export interface BuildProfilesResult {
  profiles: Profile[];
  /** Merged map, ready to save. Existing entries win so hand-edited names survive a re-import. */
  displayNames: Record<string, string>;
  displayNamesChanged: boolean;
}

/**
 * Turn a selection into profiles, generating unique credential section names.
 *
 * Section names must be unique across ALL profiles, not just the ones being created — two profiles
 * writing the same `~/.aws/credentials` section would silently overwrite each other.
 */
export function buildProfiles(input: BuildProfilesInput): BuildProfilesResult {
  const {
    accounts,
    selected,
    source,
    existingProfiles,
    existingDisplayNames,
    refreshIntervalMinutes,
    hasStoredCredentials,
  } = input;

  const taken = new Set(
    existingProfiles
      .map((p) => (p.credentialProfileName || p.name || '').trim().toLowerCase())
      .filter(Boolean)
  );

  const displayNames = { ...existingDisplayNames };
  let displayNamesChanged = false;
  const profiles: Profile[] = [];

  for (const account of accounts) {
    for (const role of account.roles) {
      if (!selected.has(role.key)) continue;

      const baseName = `${account.accountName} ${role.roleName}`;
      const base = toCredentialSectionName(baseName) || toCredentialSectionName(account.accountId);
      let section = base;
      let n = 2;
      while (taken.has(section)) section = `${base}-${n++}`;
      taken.add(section);

      const common = {
        id: uuidv4(),
        name: baseName,
        label: account.email ?? '',
        credentialProfileName: section,
        roleDisplayText: displayTextFor(account, role.roleName, displayNames),
        // Off by default: importing a dozen accounts should not silently start a dozen recurring
        // background sign-ins. The user turns it on per profile for the ones they actually use.
        autoRefresh: false,
        refreshIntervalMinutes,
        // Only claim to use default credentials when some are actually stored; otherwise the
        // profile promises a silent refresh it cannot deliver and prompts on every refresh.
        useDefaultCredentials: source.kind === 'saml' && hasStoredCredentials === true,
      };

      profiles.push(
        source.kind === 'identityCenter'
          ? {
              ...common,
              idpEntryUrl: '',
              authType: 'identityCenter',
              ssoStartUrl: source.startUrl,
              ssoRegion: source.region,
              ssoAccountId: account.accountId,
              ssoRoleName: role.roleName,
            }
          : {
              ...common,
              idpEntryUrl: source.idpEntryUrl,
              authType: 'saml',
              roleArn: role.saml?.roleArn,
              principalArn: role.saml?.principalArn,
            }
      );

      // Fill-if-empty: a name the user edited by hand must survive re-importing.
      if (!displayNames[account.accountId]?.trim() && account.accountName !== account.accountId) {
        displayNames[account.accountId] = account.accountName;
        displayNamesChanged = true;
      }
    }
  }

  return { profiles, displayNames, displayNamesChanged };
}

/** Total selectable role count, for "select all" and the summary line. */
export function countRoles(accounts: ImportableAccount[]): number {
  return accounts.reduce((n, a) => n + a.roles.length, 0);
}

export function allRoleKeys(accounts: ImportableAccount[]): Set<string> {
  return new Set(accounts.flatMap((a) => a.roles.map((r) => r.key)));
}
