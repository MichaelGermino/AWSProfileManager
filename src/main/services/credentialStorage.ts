import crypto from 'crypto';
import { shell } from 'electron';
import path from 'path';
import { validateMasterPassword } from '../../shared/masterPassword';
import { getSettings, saveSettings } from './settingsService';
import { getProfiles } from './profileStorage';

const SERVICE_NAME = 'AWSProfileManager';
export const DEFAULT_CREDENTIALS_ID = '__default__';
/**
 * Keytar account holding a verifier: a known plaintext encrypted under the master password.
 *
 * Unlock proves a password by decrypting something. Before this existed the only candidates were
 * IdP credentials and SSO sessions, so a user with a master password but nothing stored yet could
 * not be verified — and getMasterPasswordStatus responded by silently switching their master
 * password off. That is reachable for an Identity-Center-only user who sets a password and
 * restarts before signing in, or whose session could not be persisted.
 *
 * The verifier is created alongside the master password so there is always something to check
 * against, independent of what else is stored.
 *
 * NOTE (see docs/ai-constraints.md): this is a Keytar account outside the `p.id` scheme, so it has
 * to be handled explicitly in getMasterPasswordStatus, createMasterPassword,
 * unlockWithMasterPassword and forgetAllCredentialsAndResetMasterPassword. Anything keyed
 * differently is silently skipped by those loops and left behind on reset.
 */
const MASTER_PASSWORD_VERIFIER_ID = '__master_password_verifier__';
/** Plaintext inside the verifier. Not secret — decrypting it successfully is the whole signal. */
const VERIFIER_PLAINTEXT = 'aws-profile-manager:master-password-verifier:v1';

const ENC_VERSION = 'v1:';
const SALT_LEN = 16;
const IV_LEN = 12;
const TAG_LEN = 16;
const KEY_LEN = 32;
const PBKDF2_ITERATIONS = 100000;

/** Master password in memory for the session; never persisted. */
let sessionMasterPassword: string | null = null;

function getKeytar(): typeof import('keytar') | null {
  try {
    return require('keytar');
  } catch {
    return null;
  }
}

function deriveKey(password: string, salt: Buffer): Buffer {
  return crypto.pbkdf2Sync(password, salt, PBKDF2_ITERATIONS, KEY_LEN, 'sha256');
}

/** Encrypt plaintext with password; returns ENC_VERSION + base64(salt + iv + ciphertext + tag). */
function encryptPayload(password: string, plaintext: string): string {
  const salt = crypto.randomBytes(SALT_LEN);
  const iv = crypto.randomBytes(IV_LEN);
  const key = deriveKey(password, salt);
  const cipher = crypto.createCipheriv('aes-256-gcm', key, iv);
  const enc = Buffer.concat([cipher.update(plaintext, 'utf8'), cipher.final()]);
  const tag = cipher.getAuthTag();
  return ENC_VERSION + Buffer.concat([salt, iv, enc, tag]).toString('base64');
}

/** Decrypt ENC_VERSION + base64(...) with password; returns plaintext. Throws on wrong password or corrupt data. */
function decryptPayload(password: string, encoded: string): string {
  if (!encoded.startsWith(ENC_VERSION)) throw new Error('Invalid format');
  const raw = Buffer.from(encoded.slice(ENC_VERSION.length), 'base64');
  if (raw.length < SALT_LEN + IV_LEN + TAG_LEN) throw new Error('Payload too short');
  const salt = raw.subarray(0, SALT_LEN);
  const iv = raw.subarray(SALT_LEN, SALT_LEN + IV_LEN);
  const tag = raw.subarray(raw.length - TAG_LEN);
  const ciphertext = raw.subarray(SALT_LEN + IV_LEN, raw.length - TAG_LEN);
  const key = deriveKey(password, salt);
  const decipher = crypto.createDecipheriv('aes-256-gcm', key, iv);
  decipher.setAuthTag(tag);
  return decipher.update(ciphertext) + decipher.final('utf8');
}

function isEncrypted(value: string | null): boolean {
  return typeof value === 'string' && value.startsWith(ENC_VERSION);
}

/**
 * Stored SSO session ciphertext, for the master-password checks below.
 *
 * Dynamic import: ssoTokenStore imports protectSecret from this module, so a static import would
 * be a cycle. Same pattern as the refreshScheduler import further down.
 */
async function ssoSessionBlobs(): Promise<string[]> {
  try {
    const { getStoredSessionBlobs } = await import('./ssoTokenStore');
    return getStoredSessionBlobs();
  } catch {
    return [];
  }
}

/** Delete every stored SSO session. Used when the master password changes or is reset: cheaper and
 *  safer than re-encrypting, and costs the user only one browser sign-in. */
async function deleteAllSsoSessions(): Promise<void> {
  try {
    const { clearAllSessions } = await import('./ssoTokenStore');
    clearAllSessions();
  } catch {
    // best effort
  }
}

/**
 * Encrypt a secret for storage if a master password is active, otherwise hand back the plaintext.
 * Used by the SSO token store, whose caller adds OS-level encryption (safeStorage) on top either
 * way — so "plaintext" here never means plaintext at rest.
 *
 * Returns `locked` when a master password is enabled but the session has not been unlocked — the
 * caller must not silently downgrade to plaintext in that case.
 */
export function protectSecret(plaintext: string): { ok: true; blob: string } | { ok: false; locked: true } {
  const settings = getSettings();
  if (!settings.masterPasswordEnabled) return { ok: true, blob: plaintext };
  if (!sessionMasterPassword) return { ok: false, locked: true };
  return { ok: true, blob: encryptPayload(sessionMasterPassword, plaintext) };
}

/** Inverse of protectSecret. Returns null when locked, wrong key, or corrupt. */
export function unprotectSecret(blob: string | null): string | null {
  if (blob === null) return null;
  if (!isEncrypted(blob)) return blob;
  if (!sessionMasterPassword) return null;
  try {
    return decryptPayload(sessionMasterPassword, blob);
  } catch {
    return null;
  }
}

/**
 * Synchronous, side-effect-free lock check for callers that cannot await — notably the tray menu,
 * which is built inline and is reachable while the renderer is still sitting on the unlock screen.
 *
 * Deliberately more conservative than getMasterPasswordStatus(): it reports locked whenever a
 * master password is enabled and this session has not supplied it, without the Keytar/SSO scan
 * that decides whether there is actually anything encrypted to unlock. Erring toward "locked" is
 * the safe direction for a gate, and the moment the renderer calls getMasterPasswordStatus() the
 * no-secrets case clears the flag anyway.
 */
export function isLocked(): boolean {
  return getSettings().masterPasswordEnabled === true && sessionMasterPassword === null;
}

/** Status for the renderer: what to show (unlock vs create master password vs unlocked). */
export async function getMasterPasswordStatus(): Promise<
  { needsUnlock: true } | { needsCreateMasterPassword: true } | { unlocked: true }
> {
  const settings = getSettings();
  if (settings.masterPasswordEnabled) {
    if (sessionMasterPassword) return { unlocked: true };
    // Only ask for unlock if there are stored credentials to unlock
    const keytar = getKeytar();
    if (keytar) {
      // Checked first: for a user with a master password but nothing stored yet, this is the only
      // thing standing between them and having it silently switched off below.
      if (isEncrypted(await keytar.getPassword(SERVICE_NAME, MASTER_PASSWORD_VERIFIER_ID))) {
        return { needsUnlock: true };
      }
      const defaultBlob = await keytar.getPassword(SERVICE_NAME, DEFAULT_CREDENTIALS_ID);
      if (defaultBlob !== null && isEncrypted(defaultBlob)) return { needsUnlock: true };
      const profiles = getProfiles();
      for (const p of profiles) {
        const blob = await keytar.getPassword(SERVICE_NAME, p.id);
        if (blob !== null && isEncrypted(blob)) return { needsUnlock: true };
      }
    }
    // SSO sessions count too: for an Identity-Center-only user they may be the ONLY encrypted
    // blob, and without this the branch below would silently disable their master password.
    // Outside the keytar guard on purpose — SSO sessions are not stored in Keytar.
    for (const blob of await ssoSessionBlobs()) {
      if (isEncrypted(blob)) return { needsUnlock: true };
    }
    // No stored credentials to unlock; clear the flag and let them in
    saveSettings({ ...settings, masterPasswordEnabled: false });
    return { unlocked: true };
  }
  const keytar = getKeytar();
  if (!keytar) return { unlocked: true };
  const defaultPass = await keytar.getPassword(SERVICE_NAME, DEFAULT_CREDENTIALS_ID);

  /**
   * Self-heal: encrypted data but the flag says no master password.
   *
   * The flag lives in settings.json while the ciphertext lives in Keytar and sso-sessions.json, so
   * anything that reverts settings (a stale write, a restored backup, hand-editing) leaves the two
   * disagreeing. Trusting the flag then makes every secret silently unreadable: stored credentials
   * look absent so refresh prompts, and the SSO session AND its client registration look absent so
   * AWS shows its consent screen again. The ciphertext is the authority, so restore the flag and
   * ask the user to unlock.
   */
  const encryptedBlobs: (string | null)[] = [defaultPass];
  encryptedBlobs.push(await keytar.getPassword(SERVICE_NAME, MASTER_PASSWORD_VERIFIER_ID));
  for (const p of getProfiles()) encryptedBlobs.push(await keytar.getPassword(SERVICE_NAME, p.id));
  encryptedBlobs.push(...(await ssoSessionBlobs()));
  if (encryptedBlobs.some((b) => isEncrypted(b))) {
    saveSettings({ ...settings, masterPasswordEnabled: true });
    return { needsUnlock: true };
  }

  if (defaultPass !== null && !defaultPass.startsWith(ENC_VERSION)) return { needsCreateMasterPassword: true };
  const profiles = getProfiles();
  for (const p of profiles) {
    const v = await keytar.getPassword(SERVICE_NAME, p.id);
    if (v !== null && !v.startsWith(ENC_VERSION)) return { needsCreateMasterPassword: true };
  }
  return { unlocked: true };
}

/** Create master password (twice to confirm), encrypt existing legacy creds, set flag. */
export async function createMasterPassword(password: string, confirmPassword: string): Promise<{ success: true } | { success: false; error: string }> {
  if (password !== confirmPassword) return { success: false, error: 'Passwords do not match' };
  const validationError = validateMasterPassword(password);
  if (validationError) return { success: false, error: validationError };
  const keytar = getKeytar();
  if (!keytar) return { success: false, error: 'Credential storage is not available' };

  const toEncrypt: { account: string; username: string; password: string }[] = [];

  const defaultPass = await keytar.getPassword(SERVICE_NAME, DEFAULT_CREDENTIALS_ID);
  const defaultUser = await keytar.getPassword(SERVICE_NAME, `${DEFAULT_CREDENTIALS_ID}_username`);
  if ((defaultPass !== null || defaultUser !== null) && (defaultPass === null || !defaultPass.startsWith(ENC_VERSION))) {
    toEncrypt.push({
      account: DEFAULT_CREDENTIALS_ID,
      username: defaultUser ?? '',
      password: defaultPass ?? '',
    });
  }

  for (const { account, username, password: pw } of toEncrypt) {
    const blob = encryptPayload(password, JSON.stringify({ username, password: pw }));
    await keytar.setPassword(SERVICE_NAME, account, blob);
    await keytar.deletePassword(SERVICE_NAME, `${DEFAULT_CREDENTIALS_ID}_username`);
  }

  // Existing SSO sessions were stored unencrypted; drop them rather than re-encrypting.
  // Costs one browser sign-in and avoids a migration path.
  await deleteAllSsoSessions();

  // Written before the flag flips: if this throws, the app must not be left claiming a master
  // password it cannot verify.
  await keytar.setPassword(
    SERVICE_NAME,
    MASTER_PASSWORD_VERIFIER_ID,
    encryptPayload(password, VERIFIER_PLAINTEXT)
  );

  const settings = getSettings();
  saveSettings({ ...settings, masterPasswordEnabled: true });
  sessionMasterPassword = password;
  return { success: true };
}


/** Unlock with master password; verify by decrypting one stored blob. */
export async function unlockWithMasterPassword(password: string): Promise<{ success: true } | { success: false; error: string }> {
  const keytar = getKeytar();
  if (!keytar) return { success: false, error: 'Credential storage is not available' };
  // The verifier is the only candidate guaranteed to exist, so try it first.
  let blob: string | null = await keytar.getPassword(SERVICE_NAME, MASTER_PASSWORD_VERIFIER_ID);
  if (blob === null || !blob.startsWith(ENC_VERSION)) blob = await keytar.getPassword(SERVICE_NAME, DEFAULT_CREDENTIALS_ID);
  if (blob === null || !blob.startsWith(ENC_VERSION)) {
    const profiles = getProfiles();
    for (const p of profiles) {
      blob = await keytar.getPassword(SERVICE_NAME, p.id);
      if (blob !== null && blob.startsWith(ENC_VERSION)) break;
    }
  }
  if (blob === null || !blob.startsWith(ENC_VERSION)) {
    // An Identity-Center-only user has no IdP credentials; verify against an SSO session instead.
    for (const candidate of await ssoSessionBlobs()) {
      if (candidate.startsWith(ENC_VERSION)) {
        blob = candidate;
        break;
      }
    }
  }
  if (blob === null || !blob.startsWith(ENC_VERSION)) return { success: false, error: 'No stored credentials to unlock' };
  try {
    decryptPayload(password, blob);
  } catch {
    return { success: false, error: 'Wrong password' };
  }
  sessionMasterPassword = password;
  // Backfill for master passwords created before the verifier existed, so they gain the same
  // guarantee without needing to be re-created.
  try {
    const existing = await keytar.getPassword(SERVICE_NAME, MASTER_PASSWORD_VERIFIER_ID);
    if (!isEncrypted(existing)) {
      await keytar.setPassword(
        SERVICE_NAME,
        MASTER_PASSWORD_VERIFIER_ID,
        encryptPayload(password, VERIFIER_PLAINTEXT)
      );
    }
  } catch {
    // Non-fatal: the unlock itself already succeeded.
  }
  return { success: true };
}

/** Remove all stored credentials and clear master password flag; user must re-enter IdP creds and can set a new master password. */
export async function forgetAllCredentialsAndResetMasterPassword(): Promise<void> {
  const keytar = getKeytar();
  sessionMasterPassword = null;
  const settings = getSettings();
  saveSettings({ ...settings, masterPasswordEnabled: false });

  if (keytar) {
    await keytar.deletePassword(SERVICE_NAME, MASTER_PASSWORD_VERIFIER_ID);
    await keytar.deletePassword(SERVICE_NAME, DEFAULT_CREDENTIALS_ID);
    await keytar.deletePassword(SERVICE_NAME, `${DEFAULT_CREDENTIALS_ID}_username`);
    const profiles = getProfiles();
    for (const p of profiles) {
      await keytar.deletePassword(SERVICE_NAME, p.id);
      await keytar.deletePassword(SERVICE_NAME, `${p.id}_username`);
    }
  }

  /**
   * Outside the keytar guard on purpose: SSO sessions live in sso-sessions.json, not Keytar, so
   * gating them on Keytar being importable meant a machine without it kept full AWS access after
   * the user asked to forget everything.
   *
   * Clearing the browser partitions is what actually forces a re-authentication. Deleting the
   * tokens alone leaves the embedded browser still signed in to the IdP, so the next refresh sails
   * through with only AWS's consent screen — no password, no MFA. Partitions are cleared FIRST
   * because their names come from the session file this is about to delete.
   */
  try {
    const { clearAllAuthPartitions } = await import('./identityCenterService');
    await clearAllAuthPartitions();
  } catch {
    // best effort — dropping the sessions below still revokes this app's access
  }
  await deleteAllSsoSessions();
}

export async function getStoredCredentials(profileId: string): Promise<{ username: string; password: string } | null> {
  const keytar = getKeytar();
  if (!keytar) return null;

  const raw = await keytar.getPassword(SERVICE_NAME, profileId);
  const settings = getSettings();

  if (settings.masterPasswordEnabled) {
    if (!sessionMasterPassword) return null;
    if (raw !== null && isEncrypted(raw)) {
      try {
        const dec = decryptPayload(sessionMasterPassword, raw);
        const obj = JSON.parse(dec) as { username: string; password: string };
        return { username: obj.username ?? '', password: obj.password ?? '' };
      } catch {
        return null;
      }
    }
  }

  if (raw !== null && isEncrypted(raw)) return null;
  const username = await keytar.getPassword(SERVICE_NAME, `${profileId}_username`);
  if (profileId === DEFAULT_CREDENTIALS_ID) {
    if (username === null && raw === null) return null;
    return { username: username ?? '', password: raw ?? '' };
  }
  if (!raw) return null;
  return { username: username ?? '', password: raw };
}

/** Returns only the default username (for display in Settings). Never returns password. When locked, returns locked indicator. */
export async function getDefaultCredentialsDisplay(): Promise<{ username: string; hasPassword: boolean; locked?: boolean } | null> {
  const keytar = getKeytar();
  if (!keytar) return null;
  const settings = getSettings();

  if (settings.masterPasswordEnabled) {
    if (!sessionMasterPassword) {
      const raw = await keytar.getPassword(SERVICE_NAME, DEFAULT_CREDENTIALS_ID);
      if (raw !== null && isEncrypted(raw)) return { username: '', hasPassword: true, locked: true };
      return null;
    }
    const raw = await keytar.getPassword(SERVICE_NAME, DEFAULT_CREDENTIALS_ID);
    if (raw === null || !isEncrypted(raw)) {
      const username = await keytar.getPassword(SERVICE_NAME, `${DEFAULT_CREDENTIALS_ID}_username`);
      const hasPassword = !!(await keytar.getPassword(SERVICE_NAME, DEFAULT_CREDENTIALS_ID));
      if (username === null && !hasPassword) return null;
      return { username: username ?? '', hasPassword };
    }
    try {
      const dec = decryptPayload(sessionMasterPassword, raw);
      const obj = JSON.parse(dec) as { username: string; password: string };
      return { username: obj.username ?? '', hasPassword: !!(obj.password && obj.password.length > 0) };
    } catch {
      return null;
    }
  }

  const username = await keytar.getPassword(SERVICE_NAME, `${DEFAULT_CREDENTIALS_ID}_username`);
  const hasPassword = !!(await keytar.getPassword(SERVICE_NAME, DEFAULT_CREDENTIALS_ID));
  if (username === null && !hasPassword) return null;
  return { username: username ?? '', hasPassword };
}

export const MASTER_PASSWORD_REQUIRED = 'MASTER_PASSWORD_REQUIRED' as const;

/** Save default credentials. Password can be empty (username-only). Leave password empty to keep existing.
 * Returns { success: false, error: MASTER_PASSWORD_REQUIRED } when master password is not set; caller must prompt to create one. */
export async function setDefaultCredentials(
  username: string,
  password: string | null
): Promise<void | { success: false; error: typeof MASTER_PASSWORD_REQUIRED }> {
  const keytar = getKeytar();
  if (!keytar) return;
  const settings = getSettings();

  if (!settings.masterPasswordEnabled || !sessionMasterPassword) {
    return { success: false, error: MASTER_PASSWORD_REQUIRED };
  }

  let payload = { username, password: '' };
  const existing = await getStoredCredentials(DEFAULT_CREDENTIALS_ID);
  if (existing) payload = { username, password: password !== null ? password : existing.password };
  else if (password !== null) payload.password = password;
  const blob = encryptPayload(sessionMasterPassword, JSON.stringify(payload));
  await keytar.setPassword(SERVICE_NAME, DEFAULT_CREDENTIALS_ID, blob);
  await keytar.deletePassword(SERVICE_NAME, `${DEFAULT_CREDENTIALS_ID}_username`);
}

export async function forgetDefaultCredentials(): Promise<void> {
  await deleteStoredCredentials(DEFAULT_CREDENTIALS_ID);
}

export async function deleteStoredCredentials(profileId: string): Promise<void> {
  const keytar = getKeytar();
  if (!keytar) return;
  await keytar.deletePassword(SERVICE_NAME, profileId);
  await keytar.deletePassword(SERVICE_NAME, `${profileId}_username`);
}

/** Per-profile IdP credentials are not used; only default Keytar entry stores IdP username/password. */
export async function getCredentialsStatus(): Promise<{ profileId: string; hasCredentials: boolean }[]> {
  const profiles = getProfiles();
  return profiles.map((p) => ({ profileId: p.id, hasCredentials: false }));
}

export async function openCredentialsFile(): Promise<void> {
  const credentialsPath = path.join(process.env.USERPROFILE || '', '.aws', 'credentials');
  await shell.openPath(credentialsPath);
}
