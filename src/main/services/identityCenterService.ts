import https from 'https';
import http from 'http';
import crypto from 'crypto';
import { BrowserWindow, shell } from 'electron';
import { getEnterpriseHttpsAgent } from './enterpriseTls';
import { getHttpUserAgent } from './enterpriseTls';
import { normalizeStartUrl, orgKey, type SsoOrg } from '../../shared/ssoOrg';
import { ssoKeytarAccount } from './ssoOrgKey';
import { getWindowIconPath } from './appIcon';
import {
  clearSession,
  clearTokenKeepRegistration,
  isRegistrationUsable,
  isTokenUsable,
  readSession,
  writeSession,
  type SsoRegistration,
  type SsoToken,
} from './ssoTokenStore';
import { getSettings } from './settingsService';
import type { SsoAccount, SsoLoginProgress, SsoSessionStatus } from '../../shared/types';

/**
 * IAM Identity Center (Entra-federated) authentication.
 *
 * Talks to the sso-oidc and portal.sso REST APIs directly over node:https rather than through
 * @aws-sdk/client-sso{,-oidc}. The surface is six calls, and adding SDK packages would disturb
 * the load-bearing `overrides` block in package.json (see CLAUDE.md) for no functional gain.
 * The enterprise TLS agent is attached explicitly, exactly as buildStsConfig does for STS.
 *
 * Flow: RegisterClient (cached ~90d) → authorization code + PKCE in a browser → CreateToken →
 * access token (1h) + refresh token. Renewal via the refresh token is headless; only a dead
 * refresh token forces a browser trip.
 */

const CLIENT_NAME = 'AWSProfileManager';
const SCOPES = ['sso:account:access'];
const PORT_RANGE: [number, number] = [54321, 54331];
/** The user has to complete an Entra sign-in plus MFA in here. */
const LOGIN_TIMEOUT_MS = 5 * 60_000;

function oidcHost(region: string): string {
  return `oidc.${region}.amazonaws.com`;
}
function portalHost(region: string): string {
  return `portal.sso.${region}.amazonaws.com`;
}

// ---------------------------------------------------------------- transport

interface HttpResult {
  status: number;
  json: Record<string, unknown> | null;
  text: string;
}

/**
 * The smithy/undici agents ignore https.globalAgent, and so would a bare https.request here if we
 * let it default. Attach the enterprise agent so TLS-inspecting proxies (Zscaler et al) work.
 */
function requestJson(
  host: string,
  path: string,
  options: { method?: string; body?: unknown; headers?: Record<string, string> } = {}
): Promise<HttpResult> {
  const { method = 'GET', body, headers = {} } = options;
  return new Promise((resolve, reject) => {
    const payload = body === undefined ? null : JSON.stringify(body);
    const agent = getEnterpriseHttpsAgent() ?? undefined;
    const req = https.request(
      {
        host,
        path,
        method,
        agent,
        headers: {
          accept: 'application/json',
          'user-agent': getHttpUserAgent(),
          ...(payload
            ? { 'content-type': 'application/json', 'content-length': Buffer.byteLength(payload) }
            : {}),
          ...headers,
        },
      },
      (res) => {
        const chunks: Buffer[] = [];
        res.on('data', (d: Buffer) => chunks.push(d));
        res.on('end', () => {
          const text = Buffer.concat(chunks).toString('utf8');
          let json: Record<string, unknown> | null = null;
          try {
            json = text ? (JSON.parse(text) as Record<string, unknown>) : null;
          } catch {
            // Non-JSON body (proxy error page, HTML 403). Keep the text for the message.
          }
          resolve({ status: res.statusCode ?? 0, json, text });
        });
      }
    );
    req.on('error', reject);
    if (payload) req.write(payload);
    req.end();
  });
}

export class SsoApiError extends Error {
  readonly code: string;
  readonly status: number;
  constructor(message: string, code: string, status: number) {
    super(message);
    this.name = 'SsoApiError';
    this.code = code;
    this.status = status;
  }
}

function expectOk(res: HttpResult, what: string): Record<string, unknown> {
  if (res.status >= 200 && res.status < 300) return res.json ?? {};
  const code = String(res.json?.error ?? res.json?.__type ?? `HTTP ${res.status}`);
  const description = String(res.json?.error_description ?? res.json?.message ?? '');
  const detail = description ? ` — ${description}` : res.text ? ` — ${res.text.slice(0, 200)}` : '';
  throw new SsoApiError(`${what} failed: ${code}${detail}`, code, res.status);
}

// ---------------------------------------------------------------- PKCE

const b64url = (buf: Buffer): string =>
  buf.toString('base64').replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '');

function makePkce(): { verifier: string; challenge: string } {
  const verifier = b64url(crypto.randomBytes(32));
  const challenge = b64url(crypto.createHash('sha256').update(verifier).digest());
  return { verifier, challenge };
}

// ---------------------------------------------------------------- loopback listener

interface CallbackServer {
  port: number;
  redirectUri: string;
  received: Promise<Record<string, string>>;
  close: () => void;
}

/**
 * A cached registration pins the redirect URI, so when one exists we must bind that exact port
 * and cannot roam. Without a cached registration, take the first free port in the range.
 */
function startCallbackServer(fixedPort?: number): Promise<CallbackServer> {
  return new Promise((resolve, reject) => {
    let port = fixedPort ?? PORT_RANGE[0];

    const attempt = (): void => {
      let settle: (params: Record<string, string>) => void = () => {};
      const received = new Promise<Record<string, string>>((r) => {
        settle = r;
      });

      const server = http.createServer((req, res) => {
        const url = new URL(req.url ?? '/', `http://127.0.0.1:${port}`);
        if (url.pathname !== '/oauth/callback') {
          res.writeHead(404).end();
          return;
        }
        const params = Object.fromEntries(url.searchParams);
        res.writeHead(200, { 'content-type': 'text/html; charset=utf-8' });
        res.end(
          '<!doctype html><meta charset="utf-8"><title>Sign-in complete</title>' +
            '<body style="font:15px system-ui;padding:3rem;text-align:center;color:#222">' +
            `<h2>${params.error ? 'Sign-in failed' : 'Signed in'}</h2>` +
            `<p>${
              params.error
                ? String(params.error_description ?? params.error)
                : 'You can close this window and return to AWS Profile Manager.'
            }</p>`
        );
        settle(params);
      });

      server.on('error', (err: NodeJS.ErrnoException) => {
        if (err.code === 'EADDRINUSE' && fixedPort === undefined && port < PORT_RANGE[1]) {
          port += 1;
          attempt();
        } else {
          reject(err);
        }
      });

      server.listen(port, '127.0.0.1', () => {
        resolve({
          port,
          redirectUri: `http://127.0.0.1:${port}/oauth/callback`,
          received,
          close: () => {
            try {
              server.close();
            } catch {
              // already closed
            }
          },
        });
      });
    };

    attempt();
  });
}

// ---------------------------------------------------------------- OIDC calls

async function registerClient(org: SsoOrg, redirectUri: string): Promise<SsoRegistration> {
  const res = expectOk(
    await requestJson(oidcHost(org.region), '/client/register', {
      method: 'POST',
      body: {
        clientName: CLIENT_NAME,
        clientType: 'public',
        scopes: SCOPES,
        grantTypes: ['authorization_code', 'refresh_token'],
        redirectUris: [redirectUri],
        issuerUrl: org.startUrl,
      },
    }),
    'RegisterClient'
  );
  return {
    clientId: String(res.clientId),
    clientSecret: String(res.clientSecret),
    clientSecretExpiresAt: Number(res.clientSecretExpiresAt),
    redirectUri,
  };
}

async function createToken(org: SsoOrg, body: Record<string, unknown>): Promise<SsoToken & { identity?: string }> {
  const res = expectOk(
    await requestJson(oidcHost(org.region), '/token', { method: 'POST', body }),
    'CreateToken'
  );
  const expiresIn = Number(res.expiresIn ?? 3600);
  return {
    accessToken: String(res.accessToken),
    refreshToken: res.refreshToken ? String(res.refreshToken) : undefined,
    expiresAt: Date.now() + expiresIn * 1000,
    identity: typeof res.idToken === 'string' ? identityFromIdToken(res.idToken) : undefined,
  };
}

/** Best-effort: read the subject/email claim so the UI can show which account signed in. */
function identityFromIdToken(idToken: string): string | undefined {
  try {
    const payload = idToken.split('.')[1];
    if (!payload) return undefined;
    const decoded = JSON.parse(Buffer.from(payload, 'base64').toString('utf8')) as Record<string, unknown>;
    const value = decoded.email ?? decoded.preferred_username ?? decoded.sub;
    return typeof value === 'string' ? value : undefined;
  } catch {
    return undefined;
  }
}

const bearer = (accessToken: string): Record<string, string> => ({
  'x-amz-sso_bearer_token': accessToken,
});

// ---------------------------------------------------------------- interactive sign-in

let progressSink: ((progress: SsoLoginProgress) => void) | null = null;
export function setSsoProgressSink(sink: ((progress: SsoLoginProgress) => void) | null): void {
  progressSink = sink;
}
function emitProgress(progress: SsoLoginProgress): void {
  progressSink?.(progress);
}

let parentWindowRef: BrowserWindow | null = null;
export function setParentWindowForSso(win: BrowserWindow | null): void {
  parentWindowRef = win;
}

/**
 * Open the authorize URL for the user to complete.
 *
 * Default is an embedded BrowserWindow on a per-org **persistent** partition. That matters for
 * organizations that issue separate day-to-day and privileged (SA) identities: the normal browser
 * session is signed in as the wrong identity, so shell.openExternal would silently authenticate
 * the wrong account. A dedicated partition is isolated like a private window but — unlike one —
 * keeps the session across app restarts, which is what makes repeat sign-ins ~1s and click-free.
 *
 * Falls back to the external browser when settings.ssoBrowserMode is 'external', for tenants whose
 * Conditional Access policy rejects embedded webviews.
 */
function openAuthorizeUrl(
  org: SsoOrg,
  authorizeUrl: string
): { close: () => void; closedByUser: Promise<void> } {
  const mode = getSettings().ssoBrowserMode ?? 'embedded';

  if (mode === 'external') {
    void shell.openExternal(authorizeUrl);
    // No window of ours to watch, so this can never settle.
    return { close: () => {}, closedByUser: new Promise<void>(() => {}) };
  }

  const iconPath = getWindowIconPath();
  const win = new BrowserWindow({
    width: 520,
    height: 720,
    parent: parentWindowRef ?? undefined,
    autoHideMenuBar: true,
    title: 'Sign in to AWS',
    ...(iconPath && { icon: iconPath }),
    webPreferences: {
      partition: `persist:${ssoKeytarAccount(org)}`,
      nodeIntegration: false,
      contextIsolation: true,
    },
  });
  void win.loadURL(authorizeUrl);

  // Closing the window is how a user says "not now" — without watching for it they would sit on
  // "waiting for browser" until LOGIN_TIMEOUT_MS. `closing` guards against our own close() call
  // after a successful callback being mistaken for the user giving up.
  let closing = false;
  const closedByUser = new Promise<void>((resolve) => {
    win.once('closed', () => {
      if (!closing) resolve();
    });
  });

  return {
    close: () => {
      closing = true;
      if (!win.isDestroyed()) win.close();
    },
    closedByUser,
  };
}

async function interactiveLogin(org: SsoOrg): Promise<SsoToken & { identity?: string }> {
  const existing = await readSession(org);
  const cachedReg = isRegistrationUsable(existing?.registration) ? existing?.registration : undefined;

  // Reuse the registered port when we have a registration, otherwise take any free one.
  const cachedPort = cachedReg ? Number(new URL(cachedReg.redirectUri).port) : undefined;
  let server: CallbackServer;
  try {
    server = await startCallbackServer(cachedPort);
  } catch (err) {
    if (cachedPort !== undefined) {
      // Port taken by something else; re-register on a fresh port rather than failing.
      server = await startCallbackServer();
    } else {
      throw err;
    }
  }

  try {
    const registration =
      cachedReg && cachedReg.redirectUri === server.redirectUri
        ? cachedReg
        : await registerClient(org, server.redirectUri);

    await writeSession(org, { ...existing, registration });

    const { verifier, challenge } = makePkce();
    const state = b64url(crypto.randomBytes(16));

    // AWS uses 'scopes' (plural) here, not the OAuth-standard 'scope'.
    const authorizeUrl =
      `https://${oidcHost(org.region)}/authorize?` +
      new URLSearchParams({
        response_type: 'code',
        client_id: registration.clientId,
        redirect_uri: server.redirectUri,
        state,
        code_challenge: challenge,
        code_challenge_method: 'S256',
        scopes: SCOPES.join(' '),
      }).toString();

    emitProgress({ phase: 'opening', startUrl: org.startUrl });
    const browser = openAuthorizeUrl(org, authorizeUrl);
    emitProgress({ phase: 'waiting', startUrl: org.startUrl });

    let timer: NodeJS.Timeout | undefined;
    const timeout = new Promise<never>((_, reject) => {
      timer = setTimeout(
        () => reject(new Error('Sign-in timed out. Try again.')),
        LOGIN_TIMEOUT_MS
      );
    });
    const cancelled = browser.closedByUser.then(() => {
      throw new Error('Sign-in was cancelled.');
    });

    let params: Record<string, string>;
    try {
      params = await Promise.race([server.received, timeout, cancelled]);
    } finally {
      if (timer) clearTimeout(timer);
      browser.close();
    }

    if (params.error) {
      throw new Error(`Sign-in failed: ${params.error_description ?? params.error}`);
    }
    if (params.state !== state) {
      throw new Error('Sign-in failed: state mismatch.');
    }

    const token = await createToken(org, {
      clientId: registration.clientId,
      clientSecret: registration.clientSecret,
      grantType: 'authorization_code',
      code: params.code,
      redirectUri: server.redirectUri,
      codeVerifier: verifier,
    });

    const stored = await writeSession(org, { registration, token, identity: token.identity });
    if (!stored.persisted) {
      // The session still works for this run (it is cached in memory), but it won't survive a
      // restart. Say so rather than letting the user rediscover it as a surprise re-login.
      console.warn(
        `[sso] session not persisted (${stored.reason}); sign-in will be required again after restart`
      );
    }
    emitProgress({ phase: 'done', startUrl: org.startUrl, identity: token.identity });
    return token;
  } catch (err) {
    const message = err instanceof Error ? err.message : String(err);
    // Reset the IdP session on ANY failed attempt, so the next one starts at the account picker.
    //
    // An IdP-side rejection (AADSTS50105 for an unassigned account, conditional access, MFA
    // refusal) never reaches our callback: the user just sees an error page and closes the window,
    // which lands here as "cancelled". Leaving the partition intact meant Entra silently reused
    // the rejected identity forever, so picking the right account changed nothing and the only
    // escape was resetting the whole app.
    //
    // Awaited before rethrowing so the next attempt cannot race a half-finished wipe. The cost is
    // re-entering credentials after a cancel, which is the right trade against a dead end.
    await clearAuthPartition(org);
    emitProgress({ phase: 'failed', startUrl: org.startUrl, error: message });
    throw err;
  } finally {
    server.close();
  }
}

// ---------------------------------------------------------------- token acquisition

/**
 * One in-flight interactive login per org. Eight profiles in the same org refreshing at once must
 * produce one browser window, and one successful sign-in must satisfy all eight.
 */
const inflightLogins = new Map<string, Promise<SsoToken>>();

/**
 * Get a usable access token.
 *
 * `interactive: false` (the scheduler's mode) never opens a browser: it uses the cached token or
 * the refresh token, and returns null if neither works. The caller then reports
 * ssoLoginRequired rather than treating it as a failure.
 */
export async function getAccessToken(
  org: SsoOrg,
  options: { interactive: boolean }
): Promise<string | null> {
  // A sign-in the user is already completing counts, whatever mode we were called in. Without
  // this, a scheduler tick landing while the browser window is open reports "login required" and
  // throws a second prompt at a user who is mid-sign-in.
  const inflight = inflightLogins.get(orgKey(org));
  if (inflight) {
    try {
      return (await inflight).accessToken;
    } catch {
      return null;
    }
  }

  const session = await readSession(org);

  if (isTokenUsable(session?.token)) return session!.token!.accessToken;

  // Silent renewal — the path that keeps the scheduler headless.
  const registration = session?.registration;
  const refreshToken = session?.token?.refreshToken;
  if (isRegistrationUsable(registration) && refreshToken) {
    try {
      const token = await createToken(org, {
        clientId: registration.clientId,
        clientSecret: registration.clientSecret,
        grantType: 'refresh_token',
        refreshToken,
      });
      // Identity Center does not currently rotate the refresh token, but honor it if it ever does.
      const merged: SsoToken = {
        ...token,
        refreshToken: token.refreshToken ?? refreshToken,
      };
      await writeSession(org, { registration, token: merged, identity: session?.identity });
      return merged.accessToken;
    } catch {
      // Refresh token dead (SSO session expired) — fall through to interactive.
      await clearTokenKeepRegistration(org);
    }
  }

  if (!options.interactive) return null;

  const key = orgKey(org);
  const login = interactiveLogin(org).finally(() => inflightLogins.delete(key));
  inflightLogins.set(key, login);
  return (await login).accessToken;
}

export function hasInflightLogin(org: SsoOrg): boolean {
  return inflightLogins.has(orgKey(org));
}

// ---------------------------------------------------------------- portal API

async function portalGet(
  org: SsoOrg,
  path: string,
  accessToken: string,
  what: string
): Promise<Record<string, unknown>> {
  return expectOk(
    await requestJson(portalHost(org.region), path, { headers: bearer(accessToken) }),
    what
  );
}

/** Accounts plus the permission sets assigned to the signed-in user, sorted for a stable UI. */
export async function listAccountsWithRoles(org: SsoOrg, accessToken: string): Promise<SsoAccount[]> {
  const accounts: SsoAccount[] = [];
  let nextToken: string | undefined;

  do {
    const qs = new URLSearchParams({ max_result: '100', ...(nextToken ? { next_token: nextToken } : {}) });
    const page = await portalGet(org, `/assignment/accounts?${qs}`, accessToken, 'ListAccounts');
    const list = (page.accountList as Record<string, unknown>[] | undefined) ?? [];
    for (const a of list) {
      accounts.push({
        accountId: String(a.accountId),
        accountName: String(a.accountName ?? a.accountId),
        emailAddress: a.emailAddress ? String(a.emailAddress) : undefined,
        roles: [],
      });
    }
    nextToken = page.nextToken ? String(page.nextToken) : undefined;
  } while (nextToken);

  for (const account of accounts) {
    const qs = new URLSearchParams({ account_id: account.accountId, max_result: '100' });
    const page = await portalGet(org, `/assignment/roles?${qs}`, accessToken, 'ListAccountRoles');
    const roles = (page.roleList as Record<string, unknown>[] | undefined) ?? [];
    account.roles = roles.map((r) => String(r.roleName)).sort((a, b) => a.localeCompare(b));
  }

  // ListAccounts returns a different order on every call; sort so the picker doesn't reshuffle.
  accounts.sort((a, b) => a.accountName.localeCompare(b.accountName));
  return accounts;
}

export interface SsoRoleCredentials {
  accessKeyId: string;
  secretAccessKey: string;
  sessionToken: string;
  /** Epoch ms. */
  expiration: number;
}

export async function getRoleCredentials(
  org: SsoOrg,
  accessToken: string,
  accountId: string,
  roleName: string
): Promise<SsoRoleCredentials> {
  const qs = new URLSearchParams({ account_id: accountId, role_name: roleName });
  const res = await portalGet(org, `/federation/credentials?${qs}`, accessToken, 'GetRoleCredentials');
  const creds = res.roleCredentials as Record<string, unknown> | undefined;
  if (!creds?.accessKeyId || !creds.secretAccessKey || !creds.sessionToken) {
    throw new Error('Identity Center did not return credentials.');
  }
  return {
    accessKeyId: String(creds.accessKeyId),
    secretAccessKey: String(creds.secretAccessKey),
    sessionToken: String(creds.sessionToken),
    expiration: Number(creds.expiration),
  };
}

// ---------------------------------------------------------------- session management

export async function getSessionStatus(org: SsoOrg): Promise<SsoSessionStatus> {
  const session = await readSession(org);
  if (!session?.token) return { signedIn: false };
  const renewable = !!session.token.refreshToken && isRegistrationUsable(session.registration);
  if (!isTokenUsable(session.token) && !renewable) return { signedIn: false };
  return {
    signedIn: true,
    expiresAt: new Date(session.token.expiresAt).toISOString(),
    identity: session.identity,
  };
}

/**
 * Wipe the embedded browser's IdP session for an org, without touching the stored SSO session.
 *
 * The partition is deliberately persistent so repeat sign-ins are fast, but that also means a
 * failed attempt leaves the IdP logged in as whoever was used. Entra then reuses that session
 * silently on the next attempt, so retrying with the right account replays the *old* account's
 * error — most visibly AADSTS50105, which names a user the person never chose this time.
 */
async function clearAuthPartition(org: SsoOrg): Promise<void> {
  try {
    const { session } = await import('electron');
    await session.fromPartition(`persist:${ssoKeytarAccount(org)}`).clearStorageData();
  } catch {
    // partition may not exist yet
  }
}

/** Sign out of an org. Also clears the partition's cookies so the next sign-in starts clean. */
export async function signOut(org: SsoOrg): Promise<void> {
  await clearSession(org);
  await clearAuthPartition(org);
}

/** Explicit user-driven sign-in, used by the profile form and the import wizard. */
export async function signIn(org: SsoOrg): Promise<{ success: true; identity?: string } | { success: false; error: string }> {
  try {
    await getAccessToken(org, { interactive: true });
    const status = await getSessionStatus(org);
    return { success: true, identity: status.identity };
  } catch (err) {
    return { success: false, error: err instanceof Error ? err.message : String(err) };
  }
}

/**
 * Probe candidate regions for a start URL. The portal displays the region, so this is a
 * convenience for users whose portal page doesn't, not the primary path.
 */
export async function detectRegion(startUrlRaw: string): Promise<{ region: string } | { error: string }> {
  const startUrl = normalizeStartUrl(startUrlRaw);
  if (!startUrl) return { error: 'Enter a start URL first.' };
  const candidates = ['us-east-1', 'us-west-2', 'us-east-2', 'us-west-1'];

  for (const region of candidates) {
    try {
      const reg = await registerClient({ startUrl, region }, `http://127.0.0.1:${PORT_RANGE[0]}/oauth/callback`);
      const res = await requestJson(oidcHost(region), '/device_authorization', {
        method: 'POST',
        body: { clientId: reg.clientId, clientSecret: reg.clientSecret, startUrl },
      });
      if (res.status >= 200 && res.status < 300) return { region };
    } catch {
      // wrong region for this start URL — try the next
    }
  }
  return { error: 'Could not determine the region. Copy it from the access portal instead.' };
}
