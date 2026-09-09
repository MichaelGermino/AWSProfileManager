import { normalizeStartUrl } from './ssoOrg';
import type { Settings } from './types';

/**
 * Organization configuration — a small file an admin exports once and shares with their team so
 * new users do not have to be told URLs and account numbers by hand.
 *
 * SECURITY: this file is meant to be passed around, so it carries **no secrets**. Deliberately
 * excluded: the Open WebUI API key, IdP username/password, the master password, SSO tokens, and
 * anything from ~/.aws/credentials. Only the API *URL* is included. Anything added here in future
 * must be safe to email to a colleague.
 *
 * Shared by main and renderer, so no Node built-ins.
 */

export const ORG_CONFIG_VERSION = 1;
export const ORG_CONFIG_KIND = 'aws-profile-manager-org-config';

export interface OrgConfig {
  kind: typeof ORG_CONFIG_KIND;
  version: number;
  /** Free-text label shown when importing, e.g. "CARB AWS accounts". */
  organizationName?: string;
  idpEntryUrl?: string;
  ssoStartUrl?: string;
  ssoRegion?: string;
  openWebUiApiUrl?: string;
  openWebUiModel?: string;
  accountDisplayNames?: Record<string, string>;
}

function cleanString(value: unknown): string | undefined {
  return typeof value === 'string' && value.trim() ? value.trim() : undefined;
}

/** Keep only 12-digit account ids mapped to non-empty names; ignore anything else. */
function cleanDisplayNames(value: unknown): Record<string, string> | undefined {
  if (!value || typeof value !== 'object' || Array.isArray(value)) return undefined;
  const out: Record<string, string> = {};
  for (const [accountId, name] of Object.entries(value as Record<string, unknown>)) {
    if (!/^\d{12}$/.test(accountId)) continue;
    const clean = cleanString(name);
    if (clean) out[accountId] = clean;
  }
  return Object.keys(out).length > 0 ? out : undefined;
}

/**
 * Parse and sanitize an untrusted file. Returns null when it is not an org config at all.
 *
 * Unknown keys are dropped rather than carried through — a file from a newer version must not be
 * able to inject arbitrary settings.
 */
export function parseOrgConfig(raw: unknown): OrgConfig | null {
  if (!raw || typeof raw !== 'object' || Array.isArray(raw)) return null;
  const o = raw as Record<string, unknown>;
  if (o.kind !== ORG_CONFIG_KIND) return null;

  const ssoStartUrl = cleanString(o.ssoStartUrl);

  return {
    kind: ORG_CONFIG_KIND,
    version: typeof o.version === 'number' ? o.version : ORG_CONFIG_VERSION,
    organizationName: cleanString(o.organizationName),
    idpEntryUrl: cleanString(o.idpEntryUrl),
    ssoStartUrl: ssoStartUrl ? normalizeStartUrl(ssoStartUrl) : undefined,
    ssoRegion: cleanString(o.ssoRegion),
    openWebUiApiUrl: cleanString(o.openWebUiApiUrl),
    openWebUiModel: cleanString(o.openWebUiModel),
    accountDisplayNames: cleanDisplayNames(o.accountDisplayNames),
  };
}

/** Human-readable list of what a config provides, for the import confirmation. */
export function describeOrgConfig(config: OrgConfig): string[] {
  const lines: string[] = [];
  if (config.idpEntryUrl) lines.push('SAML IdP URL');
  if (config.ssoStartUrl) lines.push('Identity Center start URL');
  if (config.ssoRegion) lines.push(`Identity Center region (${config.ssoRegion})`);
  if (config.openWebUiApiUrl) lines.push('AI assistant URL');
  if (config.openWebUiModel) lines.push(`AI model (${config.openWebUiModel})`);
  const names = Object.keys(config.accountDisplayNames ?? {}).length;
  if (names) lines.push(`${names} account name${names === 1 ? '' : 's'}`);
  return lines;
}

export function isOrgConfigEmpty(config: OrgConfig): boolean {
  return describeOrgConfig(config).length === 0;
}

/**
 * Merge an org config into settings, fill-if-empty.
 *
 * Existing values always win (`settings.X || config.X`) and account names merge with hand-edited
 * entries kept, so importing is non-destructive and safe to offer at any time — not just on a
 * blank install. That is what lets Settings expose an Import button next to Export.
 *
 * Shared so the wizard step and the Settings button apply a file identically; they drifted apart
 * the moment there were two copies of this expression.
 */
export function applyOrgConfigToSettings(
  settings: Settings,
  config: OrgConfig,
  now: Date = new Date()
): Settings {
  return {
    ...settings,
    defaultIdpEntryUrl: settings.defaultIdpEntryUrl?.trim() || config.idpEntryUrl || '',
    defaultSsoStartUrl: settings.defaultSsoStartUrl?.trim() || config.ssoStartUrl,
    defaultSsoRegion: settings.defaultSsoRegion?.trim() || config.ssoRegion,
    openWebUiApiUrl: settings.openWebUiApiUrl?.trim() || config.openWebUiApiUrl || '',
    openWebUiModel: settings.openWebUiModel?.trim() || config.openWebUiModel || '',
    accountDisplayNames: { ...config.accountDisplayNames, ...settings.accountDisplayNames },
    orgConfigImportedAt: now.toISOString(),
    orgConfigImportedName: config.organizationName,
  };
}
