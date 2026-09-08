import fs from 'fs';
import path from 'path';
import { safeStorage } from 'electron';
import { type SsoOrg } from '../../shared/ssoOrg';
import { ssoKeytarAccount } from './ssoOrgKey';
import { protectSecret, unprotectSecret } from './credentialStorage';
import { getAppDataPath } from './profileStorage';

/**
 * Cache of Identity Center OIDC state, keyed by org (start URL + region) rather than by profile:
 * one SSO session serves every profile in the org, unlike IdP credentials which are per-profile.
 *
 * ## Why this is not in Keytar
 *
 * Windows Credential Manager caps a credential blob at 2560 bytes (CRED_MAX_CREDENTIAL_BLOB_SIZE).
 * An SSO session carries an access token and a refresh token, each commonly 1–2 KB, plus the client
 * registration — and AES+base64 adds ~35% on top. `keytar.setPassword` therefore throws, which is
 * why the IdP password (short) stores fine while an SSO session never persisted at all.
 *
 * So sessions live in a JSON file in app data, with every value encrypted by Electron's
 * `safeStorage` (DPAPI on Windows, tied to the OS user) and, when a master password is enabled,
 * wrapped in the app's own `v1:` AES-256-GCM layer first. There is deliberately **no plaintext
 * fallback**: if `safeStorage` is unavailable the session stays in memory only and is lost on exit.
 *
 * Everything here is a bearer credential and must never reach the renderer.
 */

const SESSIONS_FILENAME = 'sso-sessions.json';

export interface SsoRegistration {
  clientId: string;
  clientSecret: string;
  /** Epoch seconds, from RegisterClient. Typically ~90 days out. */
  clientSecretExpiresAt: number;
  /** Registered redirect URI; pins the loopback port for subsequent logins. */
  redirectUri: string;
}

export interface SsoToken {
  accessToken: string;
  refreshToken?: string;
  /** Epoch ms. */
  expiresAt: number;
}

export interface SsoSession {
  registration?: SsoRegistration;
  token?: SsoToken;
  /** Best-effort record of who signed in, so a wrong-account sign-in can be surfaced. */
  identity?: string;
}

/**
 * Sessions also live in memory for the life of the process. Not an optimization: it keeps the
 * feature working when the session cannot be written (locked master password, no safeStorage),
 * instead of silently falling back to a fresh interactive login on every call — which
 * re-registered the OIDC client and made AWS re-show its consent screen every time.
 */
const memorySessions = new Map<string, SsoSession>();

function sessionsFilePath(): string {
  return path.join(getAppDataPath(), SESSIONS_FILENAME);
}

function readFileMap(): Record<string, string> {
  try {
    const raw = fs.readFileSync(sessionsFilePath(), 'utf-8');
    const parsed = JSON.parse(raw) as unknown;
    if (!parsed || typeof parsed !== 'object' || Array.isArray(parsed)) return {};
    return parsed as Record<string, string>;
  } catch {
    // Missing or malformed — same contract as the other storage services: treat as empty.
    return {};
  }
}

function writeFileMap(map: Record<string, string>): boolean {
  try {
    const dir = getAppDataPath();
    if (!fs.existsSync(dir)) fs.mkdirSync(dir, { recursive: true });
    fs.writeFileSync(sessionsFilePath(), JSON.stringify(map, null, 2), 'utf-8');
    return true;
  } catch {
    return false;
  }
}

function encryptionAvailable(): boolean {
  try {
    return safeStorage.isEncryptionAvailable();
  } catch {
    return false;
  }
}

function wrap(plain: string): string | null {
  try {
    return safeStorage.encryptString(plain).toString('base64');
  } catch {
    return null;
  }
}

function unwrap(encoded: string): string | null {
  try {
    return safeStorage.decryptString(Buffer.from(encoded, 'base64'));
  } catch {
    // Written by a different OS user or a reinstalled key — unreadable, so treat as absent.
    return null;
  }
}

export async function readSession(org: SsoOrg): Promise<SsoSession | null> {
  const account = ssoKeytarAccount(org);
  const cached = memorySessions.get(account);
  if (cached) return cached;

  const stored = readFileMap()[account];
  if (!stored) return null;

  const unwrapped = unwrap(stored);
  if (!unwrapped) return null;

  // When a master password is enabled the inner value is `v1:` ciphertext; unprotectSecret
  // returns null while locked, which correctly reads as "no session".
  const plain = unprotectSecret(unwrapped);
  if (!plain) return null;

  try {
    const session = JSON.parse(plain) as SsoSession;
    memorySessions.set(account, session);
    return session;
  } catch {
    return null;
  }
}

export interface WriteSessionResult {
  /** Always true — the session is usable for this run even when it could not be persisted. */
  cached: true;
  /** False when locked, when safeStorage is unavailable, or when the file write failed. */
  persisted: boolean;
  reason?: 'locked' | 'no-encryption' | 'write-failed';
}

export async function writeSession(org: SsoOrg, session: SsoSession): Promise<WriteSessionResult> {
  const account = ssoKeytarAccount(org);
  // Cache first, unconditionally: a persistence failure must not cost the user their session.
  memorySessions.set(account, session);

  const protectedBlob = protectSecret(JSON.stringify(session));
  if (!protectedBlob.ok) return { cached: true, persisted: false, reason: 'locked' };

  if (!encryptionAvailable()) return { cached: true, persisted: false, reason: 'no-encryption' };
  const wrapped = wrap(protectedBlob.blob);
  if (!wrapped) return { cached: true, persisted: false, reason: 'no-encryption' };

  const map = readFileMap();
  map[account] = wrapped;
  return writeFileMap(map)
    ? { cached: true, persisted: true }
    : { cached: true, persisted: false, reason: 'write-failed' };
}

export async function clearSession(org: SsoOrg): Promise<void> {
  const account = ssoKeytarAccount(org);
  memorySessions.delete(account);
  const map = readFileMap();
  if (account in map) {
    delete map[account];
    writeFileMap(map);
  }
}

/** Drop every stored session. Used when the master password is created, changed, or reset. */
export function clearAllSessions(): void {
  memorySessions.clear();
  try {
    fs.rmSync(sessionsFilePath(), { force: true });
  } catch {
    // best effort — a missing file is the desired end state anyway
  }
}

/**
 * Inner (post-safeStorage) blobs for stored sessions, so credentialStorage can spot `v1:`
 * ciphertext when deciding whether a master password needs unlocking and when verifying one.
 * Returns ciphertext only — never a decrypted session.
 */
export function getStoredSessionBlobs(): string[] {
  const out: string[] = [];
  for (const encoded of Object.values(readFileMap())) {
    const unwrapped = unwrap(encoded);
    if (unwrapped) out.push(unwrapped);
  }
  return out;
}

/** Drop only the tokens, keeping the (still valid, consent-bearing) client registration. */
export async function clearTokenKeepRegistration(org: SsoOrg): Promise<void> {
  const session = await readSession(org);
  if (!session) return;
  // Keep identity: it describes who signed in, not the expired session.
  await writeSession(org, { registration: session.registration, identity: session.identity });
}

export function isRegistrationUsable(reg: SsoRegistration | undefined): reg is SsoRegistration {
  if (!reg?.clientId || !reg.clientSecret || !reg.redirectUri) return false;
  // Retire an hour early so a long-running refresh can't straddle the expiry.
  return reg.clientSecretExpiresAt * 1000 > Date.now() + 3600_000;
}

/** Access tokens last an hour; renew with a 5 minute margin so calls never race the expiry.
 *  Plain boolean rather than a type predicate: negating a predicate over an already-narrowed
 *  SsoToken collapses the else-branch to `never`. */
export function isTokenUsable(token: SsoToken | undefined): boolean {
  return !!token?.accessToken && token.expiresAt > Date.now() + 5 * 60_000;
}
