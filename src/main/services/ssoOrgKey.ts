import crypto from 'crypto';
import { orgKey, type SsoOrg } from '../../shared/ssoOrg';

/**
 * Main-only: hashed identity for an org, used as the Keytar account name and as the Electron
 * session partition name. Separate from shared/ssoOrg.ts because node:crypto cannot go through
 * the renderer bundle.
 *
 * One SSO session serves every profile in an org, which is why this keys on the org rather than
 * on profileId the way IdP credentials do.
 */
export function ssoKeytarAccount(org: SsoOrg): string {
  return `sso:${crypto.createHash('sha256').update(orgKey(org)).digest('hex').slice(0, 32)}`;
}
