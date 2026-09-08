import type { Profile, ProfileAuthType } from './types';

/**
 * Identity Center helpers shared by main AND renderer, so this module must stay free of Node
 * built-ins — it goes through Vite's bundle. The hashed Keytar/partition name lives in
 * src/main/services/ssoOrgKey.ts, which is main-only because it needs node:crypto.
 */

export interface SsoOrg {
  startUrl: string;
  region: string;
}

/** `undefined` authType means a profile created before Identity Center support existed. */
export function resolveAuthType(profile: Pick<Profile, 'authType'>): ProfileAuthType {
  return profile.authType ?? 'saml';
}

export function isIdentityCenterProfile(profile: Pick<Profile, 'authType'>): boolean {
  return resolveAuthType(profile) === 'identityCenter';
}

/**
 * The access portal shows the URL as `https://d-xxxx.awsapps.com/start/#/`, but the OIDC and
 * portal APIs reject the fragment. Normalizing on input means users can paste what they see.
 */
export function normalizeStartUrl(raw: string): string {
  return (raw ?? '').trim().replace(/\/?#.*$/, '').replace(/\/+$/, '');
}

/** Plain string identity for an org, for Map/Set keys. Not a secret and not hashed. */
export function orgKey(org: SsoOrg): string {
  return `${normalizeStartUrl(org.startUrl)}|${(org.region ?? '').trim()}`;
}

export function orgOfProfile(profile: Pick<Profile, 'ssoStartUrl' | 'ssoRegion'>): SsoOrg | null {
  const startUrl = normalizeStartUrl(profile.ssoStartUrl ?? '');
  const region = (profile.ssoRegion ?? '').trim();
  if (!startUrl || !region) return null;
  return { startUrl, region };
}

export function sameOrg(a: SsoOrg, b: SsoOrg): boolean {
  return orgKey(a) === orgKey(b);
}

/** Distinct orgs referenced by the given profiles, for enumerating stored sessions. */
export function distinctOrgs(
  profiles: Pick<Profile, 'authType' | 'ssoStartUrl' | 'ssoRegion'>[]
): SsoOrg[] {
  const byKey = new Map<string, SsoOrg>();
  for (const p of profiles) {
    if (!isIdentityCenterProfile(p)) continue;
    const org = orgOfProfile(p);
    if (org) byKey.set(orgKey(org), org);
  }
  return [...byKey.values()];
}
