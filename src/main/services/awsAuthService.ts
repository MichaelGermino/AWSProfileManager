import axios, { type AxiosResponse } from 'axios';
import { wrapper } from 'axios-cookiejar-support';
import { CookieJar } from 'tough-cookie';
import * as cheerio from 'cheerio';
import { parseStringPromise } from 'xml2js';
import { AssumeRoleWithSAMLCommand, STSClient, type STSClientConfig } from '@aws-sdk/client-sts';
import { NodeHttpHandler } from '@smithy/node-http-handler';
import { appendAuthAudit, idpHostFromUrl, maskUsername } from './authAuditLog';
import { getEnterpriseHttpsAgent } from './enterpriseTls';
import { sendToRenderer } from './ipcBridge';
import { classifyAuthFailure } from './networkErrorClassifier';
import { clearNetworkFailure, noteNetworkFailure } from './networkStatus';
import { recordConsecutiveRefreshFailure, resetConsecutiveRefreshFailures } from './refreshFailureCounters';
import { getProfileById, getProfiles, saveProfile } from './profileStorage';
import { getStoredCredentials, DEFAULT_CREDENTIALS_ID } from './credentialStorage';
import { writeCredentialsForProfile } from './credentialsFile';
import { setCachedRoles } from './rolesCache';
import type { AwsRole } from '../../shared/types';
import { BrowserWindow } from 'electron';

let mainWindowRef: BrowserWindow | null = null;
export function setMainWindowForAuth(win: BrowserWindow | null) {
  mainWindowRef = win;
}

function notify(title: string, body: string) {
  mainWindowRef?.webContents.send('notify', title, body);
}

function sendCredentialsRequired(profileId: string, prefillUsername?: string) {
  if (mainWindowRef && !mainWindowRef.isDestroyed()) {
    mainWindowRef.show();
    mainWindowRef.focus();
  }
  mainWindowRef?.webContents.send('auth:credentialsRequired', profileId, prefillUsername ?? '');
}

function sendCredentialsRefreshed(profileId: string) {
  mainWindowRef?.webContents.send('auth:credentialsRefreshed', profileId);
}

function sendRefreshStarted(profileId: string) {
  mainWindowRef?.webContents.send('auth:refreshStarted', profileId);
}

function sendCredentialsExpired(profileId: string, message: string) {
  if (mainWindowRef && !mainWindowRef.isDestroyed()) {
    mainWindowRef.show();
    mainWindowRef.focus();
  }
  mainWindowRef?.webContents.send('auth:credentialsExpired', profileId, message);
}

function sendNetworkUnavailable(profileId: string) {
  // No show/focus: offline failures shouldn't yank the window forward.
  mainWindowRef?.webContents.send('auth:networkUnavailable', profileId);
}

/**
 * Log failure, optionally count toward the consecutive-failure auto-pause, and notify UI.
 *
 * Network failures (DNS, offline, refused, timeout) are classified separately: they do NOT count
 * toward the lockout-prevention pause (no login attempt reached the IdP, so no lockout risk),
 * are not written to the auth audit log as failures, and surface a non-alarming
 * `auth:networkUnavailable` event instead of `auth:credentialsExpired`.
 */
function noteAuthFailure(options: {
  profileId: string;
  error: string;
  /** Original error (Error/AxiosError/etc.). Pass when available so error.code/cause survive classification. */
  errorObject?: unknown;
  source: string;
  notifyCredentialsExpired?: boolean;
}) {
  const kind = classifyAuthFailure(options.errorObject ?? options.error);

  if (kind === 'network') {
    noteNetworkFailure();
    if (options.notifyCredentialsExpired !== false) {
      sendNetworkUnavailable(options.profileId);
    }
    return;
  }

  appendAuthAudit({
    type: 'failure',
    source: options.source,
    profileId: options.profileId,
    error: options.error.slice(0, 2000),
  });
  if (recordConsecutiveRefreshFailure()) {
    void import('./refreshScheduler').then(({ setRefreshPaused }) => {
      setRefreshPaused(true, { dueToFailures: true });
      sendToRenderer('auth:autoRefreshPausedForFailures');
    });
  }
  if (options.notifyCredentialsExpired !== false) {
    sendCredentialsExpired(options.profileId, options.error);
  }
}

function sendRefreshAllRequired(credentialProfileIds: string[], defaultProfileIds: string[]) {
  if (mainWindowRef && !mainWindowRef.isDestroyed()) {
    mainWindowRef.show();
    mainWindowRef.focus();
  }
  mainWindowRef?.webContents.send('auth:refreshAllRequired', credentialProfileIds, defaultProfileIds);
}

/**
 * Splits profiles into those we have credentials for (stored default) vs those that need a prompt.
 * So "Refresh all" can prompt once and refresh all, regardless of useDefaultCredentials checkbox.
 */
async function splitProfilesByCredentials(
  profiles: { id: string; useDefaultCredentials?: boolean }[]
): Promise<{ needCreds: string[]; haveCreds: string[] }> {
  const needCreds: string[] = [];
  const haveCreds: string[] = [];
  let defaultStored: { username: string; password: string } | null | undefined = undefined;
  for (const p of profiles) {
    if (!p.useDefaultCredentials) {
      needCreds.push(p.id);
      continue;
    }
    if (defaultStored === undefined) {
      try {
        defaultStored = await getStoredCredentials(DEFAULT_CREDENTIALS_ID);
      } catch {
        defaultStored = null;
      }
    }
    if (defaultStored?.password?.trim()) {
      haveCreds.push(p.id);
    } else {
      needCreds.push(p.id);
    }
  }
  return { needCreds, haveCreds };
}

interface PendingAuth {
  assertion: string;
  roles: AwsRole[];
}

const pendingAuthByProfile = new Map<string, PendingAuth>();

const REGION = 'us-west-2';
const SAML_ATTR_ROLE = 'https://aws.amazon.com/SAML/Attributes/Role';

/** Build STSClient config that honors enterprise-TLS trust when an enterprise agent is
 *  active. The smithy NodeHttpHandler builds its own https.Agent by default and ignores
 *  https.globalAgent, so without this STS calls would fail behind a TLS-inspection proxy. */
function buildStsConfig(): STSClientConfig {
  const enterpriseAgent = getEnterpriseHttpsAgent();
  if (!enterpriseAgent) return { region: REGION };
  return {
    region: REGION,
    requestHandler: new NodeHttpHandler({ httpsAgent: enterpriseAgent }),
  };
}

function normalizeRoleValue(value: string): AwsRole | null {
  const chunks = value.split(',').map((s) => s.trim());
  if (chunks.length !== 2) return null;
  let roleArn: string;
  let principalArn: string;
  if (chunks[0].includes('saml-provider')) {
    roleArn = chunks[1];
    principalArn = chunks[0];
  } else {
    roleArn = chunks[0];
    principalArn = chunks[1];
  }
  if (!roleArn || !principalArn || !roleArn.startsWith('arn:') || !principalArn.startsWith('arn:')) return null;
  return { roleArn, principalArn, displayText: roleArn };
}

/** Extract role_arn,principal_arn pairs from raw SAML XML. Match Python: Attribute Name=Role, AttributeValue texts. */
function extractRolesFromSamlXml(xml: string): AwsRole[] {
  const roles: AwsRole[] = [];
  const seen = new Set<string>();

  // Find any text that looks like "arn:aws:iam::123:role/...,arn:aws:iam::123:saml-provider/..." (or reverse order)
  const arnPairRegex =
    /(arn:aws:iam::\d+:role\/[^,\s<>"&]+,\s*arn:aws:iam::\d+:saml-provider\/[^<\s"&]+|arn:aws:iam::\d+:saml-provider\/[^,\s<>"&]+,\s*arn:aws:iam::\d+:role\/[^<\s"&]+)/g;
  let m: RegExpExecArray | null;
  while ((m = arnPairRegex.exec(xml)) !== null) {
    const r = normalizeRoleValue(m[1]);
    if (r && !seen.has(r.roleArn)) {
      seen.add(r.roleArn);
      roles.push(r);
    }
  }

  return roles;
}

async function parseSamlRoles(assertionBase64: string): Promise<AwsRole[]> {
  const decoded = Buffer.from(assertionBase64, 'base64').toString('utf-8');

  // Try xml2js first (structure varies with namespace prefixes)
  try {
    const parsed = await parseStringPromise(decoded, { explicitArray: true });
    const assertion =
      parsed?.Assertion ?? parsed?.['saml:Assertion'] ?? parsed?.['saml2:Assertion'] ?? parsed?.['samlp:Response']?.Assertion;
    const ast = Array.isArray(assertion) ? assertion[0] : assertion;
    if (ast) {
      const attStatement =
        ast.AttributeStatement ?? ast['saml:AttributeStatement'] ?? ast['saml2:AttributeStatement'];
      const astList = Array.isArray(attStatement) ? attStatement : attStatement ? [attStatement] : [];
      for (const st of astList) {
        const attrs = st.Attribute ?? st['saml:Attribute'] ?? st['saml2:Attribute'];
        const attrList = Array.isArray(attrs) ? attrs : attrs ? [attrs] : [];
        for (const attr of attrList) {
          const a = typeof attr === 'object' && attr !== null ? attr : {};
          const name = a.$?.Name ?? a.$?.name ?? a.Name ?? a.name;
          if (name === SAML_ATTR_ROLE) {
            const values = a.AttributeValue ?? a['saml:AttributeValue'] ?? a['saml2:AttributeValue'];
            const valList = Array.isArray(values) ? values : values ? [values] : [];
            const roles: AwsRole[] = [];
            for (const v of valList) {
              const text = typeof v === 'string' ? v : (v._ ?? v ?? v['#text'] ?? '');
              const r = normalizeRoleValue(String(text).trim());
              if (r) roles.push(r);
            }
            if (roles.length > 0) return roles;
          }
        }
      }
    }
  } catch {
    // ignore parse errors, fall through to raw XML
  }

  return extractRolesFromSamlXml(decoded);
}

// Match Python requests.Session() behavior: follow redirects, keep cookies.
// Use a User-Agent that matches what works with this IdP (Python script uses
// requests default; some ADFS servers return different content for browsers).
const SESSION_HEADERS = {
  'User-Agent': 'python-requests/2.31.0',
  Accept: 'text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8',
};

async function performLogin(
  idpEntryUrl: string,
  username: string,
  password: string,
  meta?: { profileId?: string; source: string }
): Promise<{ assertion: string; roles: AwsRole[] }> {
  const jar = new CookieJar();
  const enterpriseAgent = getEnterpriseHttpsAgent();
  const client = wrapper(
    axios.create({
      jar,
      maxRedirects: 0,
      validateStatus: () => true,
      headers: SESSION_HEADERS,
      // Pass the enterprise-trust agent explicitly. Falls through to https.globalAgent
      // (also CA-extended in enterpriseTls) when no enterprise CAs are present.
      ...(enterpriseAgent && { httpsAgent: enterpriseAgent }),
    })
  );

  // Follow redirects manually so we know the exact URL that serves the form and cookies are set at each step
  let currentUrl = idpEntryUrl;
  let formResponse: AxiosResponse | undefined;
  for (let i = 0; i < 10; i++) {
    const res = await client.get(currentUrl) as AxiosResponse;
    formResponse = res;
    if (res.status >= 200 && res.status < 300) break;
    if (res.status === 301 || res.status === 302) {
      const loc = res.headers.location;
      if (!loc) throw new Error('Redirect response missing Location header');
      currentUrl = loc.startsWith('http') ? loc : new URL(loc, currentUrl).href;
      continue;
    }
    throw new Error(`Unexpected response ${res.status} from IdP`);
  }
  if (!formResponse) throw new Error('IdP did not return a login form');
  const finalGetUrl = currentUrl;

  // Python: BeautifulSoup(formresponse.text) then find_all(re.compile('(INPUT|input)'))
  const html = typeof formResponse.data === 'string' ? formResponse.data : String(formResponse.data);
  const $ = cheerio.load(html);

  const inputNames: string[] = [];
  $('input, INPUT').each((_, el) => {
    const name = $(el).attr('name');
    if (name) inputNames.push(name);
  });
  const safeSnippet = (s: string, max: number) =>
    s.slice(0, max).replace(/value="[^"]{50,}"/g, 'value="[REDACTED]"').replace(/password[^>]*/gi, 'password=[REDACTED]');
  console.error('[AWS Profile Manager] GET login form: status=%s, url=%s', formResponse.status, finalGetUrl);
  console.error('[AWS Profile Manager] Form input names: %s', inputNames.join(', ') || '(none)');
  console.error('[AWS Profile Manager] Page snippet (first 1200 chars):\n%s', safeSnippet(html, 1200));

  // Build payload like Python: dict so duplicate names overwrite (last wins). Python: payload[name] = value
  const payloadObj: Record<string, string> = {};
  $('input, INPUT').each((_, el) => {
    const name = $(el).attr('name');
    const value = $(el).attr('value') ?? '';
    if (!name) return;
    const n = name.toLowerCase();
    if (n.includes('user')) payloadObj[name] = username;
    else if (n.includes('email')) payloadObj[name] = username;
    else if (n.includes('pass')) payloadObj[name] = password;
    else payloadObj[name] = value;
  });
  const payload = new URLSearchParams(payloadObj);

  console.error('[AWS Profile Manager] POST to: %s', finalGetUrl);
  console.error('[AWS Profile Manager] Payload keys: %s', [...payload.keys()].join(', '));
  if (meta) {
    appendAuthAudit({
      type: 'idp_request',
      source: meta.source,
      profileId: meta.profileId,
      idpHost: idpHostFromUrl(finalGetUrl),
      usernameHint: maskUsername(username),
    });
  }
  const idpPostStartedAt = Date.now();
  const postResponse = await client.post(finalGetUrl, payload.toString(), {
    headers: {
      'Content-Type': 'application/x-www-form-urlencoded',
      Referer: finalGetUrl,
      ...SESSION_HEADERS,
    },
  });

  // ADFS often returns 302 after login; the SAML response is on the redirected page. Follow redirects.
  let samlPageResponse: AxiosResponse = postResponse as AxiosResponse;
  let redirectUrl: string = finalGetUrl;
  for (let r = 0; r < 5; r++) {
    if (samlPageResponse.status === 301 || samlPageResponse.status === 302) {
      const locRaw = samlPageResponse.headers.location;
      const loc = typeof locRaw === 'string' ? locRaw : Array.isArray(locRaw) ? locRaw[0] : undefined;
      if (!loc) break;
      redirectUrl = loc.startsWith('http') ? loc : new URL(loc, redirectUrl).href;
      samlPageResponse = await client.get(redirectUrl) as AxiosResponse;
      continue;
    }
    if (samlPageResponse.status >= 200 && samlPageResponse.status < 300) break;
    break;
  }

  const postHtml = typeof samlPageResponse.data === 'string' ? samlPageResponse.data : String(samlPageResponse.data);
  const $post = cheerio.load(postHtml);
  let assertion = '';
  $post('input').each((_, el) => {
    const name = $post(el).attr('name');
    if (name === 'SAMLResponse') assertion = $post(el).attr('value') ?? '';
  });
  if (!assertion) {
    $post('input').each((_, el) => {
      const name = $post(el).attr('name') || '';
      if (name.toLowerCase() === 'samlresponse') assertion = $post(el).attr('value') ?? '';
    });
  }

  if (!assertion) {
    const status = samlPageResponse.status;
    const len = postHtml.length;
    const hasInBody = /samlresponse/i.test(postHtml);
    console.error(
      '[AWS Profile Manager] No SAML assertion. Final response status=%s, body length=%s, body contains "SAMLResponse"=%s',
      status,
      len,
      hasInBody
    );
    console.error('[AWS Profile Manager] Response headers: %s', JSON.stringify(samlPageResponse.headers, null, 2));
    const bodyToPrint = len <= 3000 ? postHtml : postHtml.slice(0, 1500) + '\n... [truncated]';
    console.error('[AWS Profile Manager] Response body:\n%s', bodyToPrint);
    throw new Error(
      'Response did not contain a valid SAML assertion. Check your username and password, and that the IdP URL is correct.'
    );
  }

  let roles = await parseSamlRoles(assertion);
  if (roles.length === 0) {
    throw new Error('No roles were found in the SAML assertion');
  }

  if (meta) {
    appendAuthAudit({
      type: 'idp_success',
      source: meta.source,
      profileId: meta.profileId,
      idpHost: idpHostFromUrl(finalGetUrl),
      usernameHint: maskUsername(username),
      roleCount: roles.length,
      durationMs: Date.now() - idpPostStartedAt,
    });
  }

  // We only receive the SAML post form (no role-picker HTML). Friendly names come from Settings → Account display names.

  return { assertion, roles };
}

export type FetchRolesResult =
  | { roles: AwsRole[] }
  | { credentialsRequired: true; profileId?: string; prefillUsername?: string }
  | { success: false; error: string };

/** Fetch roles for an IdP (to populate role dropdown). Uses default creds if useDefaultCredentials, else returns credentialsRequired. */
export async function fetchRolesForIdp(
  idpEntryUrl: string,
  options: { useDefaultCredentials: boolean; profileId?: string }
): Promise<FetchRolesResult> {
  if (!idpEntryUrl?.trim()) {
    return { success: false, error: 'IdP entry URL is required.' };
  }
  let username: string;
  let password: string;
  if (options.useDefaultCredentials) {
    const credentialsKey = DEFAULT_CREDENTIALS_ID;
    const stored = await getStoredCredentials(credentialsKey);
    if (!stored?.password) {
      return {
        credentialsRequired: true,
        profileId: options.profileId,
        prefillUsername: stored?.username ?? '',
      };
    }
    username = stored.username;
    password = stored.password;
  } else {
    return { credentialsRequired: true, profileId: options.profileId };
  }
  try {
    const { roles } = await performLogin(idpEntryUrl, username, password, {
      source: 'fetchRolesForIdp',
      profileId: options.profileId,
    });
    resetConsecutiveRefreshFailures();
    clearNetworkFailure();
    setCachedRoles(idpEntryUrl, roles);
    return { roles };
  } catch (err) {
    const message = err instanceof Error ? err.message : String(err);
    noteAuthFailure({
      profileId: options.profileId ?? 'fetch-roles',
      error: message,
      errorObject: err,
      source: 'fetchRolesForIdp',
      notifyCredentialsExpired: false,
    });
    return { success: false, error: message };
  }
}

/** Fetch roles using provided credentials (after user was prompted for load-roles). */
export async function fetchRolesWithCredentials(
  idpEntryUrl: string,
  username: string,
  password: string
): Promise<{ roles: AwsRole[] } | { success: false; error: string }> {
  if (!idpEntryUrl?.trim()) {
    return { success: false, error: 'IdP entry URL is required.' };
  }
  try {
    const { roles } = await performLogin(idpEntryUrl, username, password, {
      source: 'fetchRolesWithCredentials',
      profileId: undefined,
    });
    resetConsecutiveRefreshFailures();
    clearNetworkFailure();
    setCachedRoles(idpEntryUrl, roles);
    return { roles };
  } catch (err) {
    const message = err instanceof Error ? err.message : String(err);
    noteAuthFailure({
      profileId: 'fetch-roles',
      error: message,
      errorObject: err,
      source: 'fetchRolesWithCredentials',
      notifyCredentialsExpired: false,
    });
    return { success: false, error: message };
  }
}

export type RefreshResult =
  | { success: true }
  | { success: false; error: string }
  | { required: true; profileId: string; prefillUsername?: string }
  | { roles: AwsRole[]; profileId: string };

export async function refreshProfile(
  profileId: string,
  overrideCredentials?: { username: string; password: string }
): Promise<RefreshResult> {
  const profile = getProfileById(profileId);
  if (!profile) {
    noteAuthFailure({
      profileId,
      error: 'Profile not found',
      source: 'refreshProfile',
      notifyCredentialsExpired: false,
    });
    return { success: false, error: 'Profile not found' };
  }
  if (!profile.idpEntryUrl?.trim()) {
    noteAuthFailure({
      profileId,
      error: 'Profile has no IdP entry URL. Edit the profile and set the IdP URL.',
      source: 'refreshProfile',
      notifyCredentialsExpired: false,
    });
    return { success: false, error: 'Profile has no IdP entry URL. Edit the profile and set the IdP URL.' };
  }

  let username: string;
  let password: string;
  if (overrideCredentials) {
    username = overrideCredentials.username;
    password = overrideCredentials.password;
  } else {
    if (!profile.useDefaultCredentials) {
      sendCredentialsRequired(profileId);
      return { required: true, profileId };
    }
    const credentialsKey = DEFAULT_CREDENTIALS_ID;
    let stored: { username: string; password: string } | null = null;
    try {
      stored = await getStoredCredentials(credentialsKey);
      // When using default credentials, only use the default entry. If it has no password, prompt every time (no fallback to profile-stored).
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      const errText = `Could not read saved credentials: ${msg}. Try entering credentials again.`;
      noteAuthFailure({ profileId, error: errText, errorObject: err, source: 'refreshProfile' });
      return {
        success: false,
        error: errText,
      };
    }
    if (!stored?.password?.trim()) {
      sendCredentialsRequired(profileId, stored?.username);
      return { required: true, profileId, prefillUsername: stored?.username ?? '' };
    }
    username = stored.username;
    password = stored.password;
  }

  sendRefreshStarted(profileId);
  try {
    const { assertion, roles } = await performLogin(profile.idpEntryUrl, username, password, {
      source: 'refreshProfile',
      profileId,
    });
    username = '';
    password = '';

    let roleArn: string;
    let principalArn: string;
    if (roles.length === 1) {
      roleArn = roles[0].roleArn;
      principalArn = roles[0].principalArn;
    } else if (profile.roleArn && profile.principalArn) {
      const match = roles.find(
        (r) => r.roleArn === profile.roleArn && r.principalArn === profile.principalArn
      );
      if (match) {
        roleArn = match.roleArn;
        principalArn = match.principalArn;
      } else {
        pendingAuthByProfile.set(profileId, { assertion, roles });
        resetConsecutiveRefreshFailures();
        clearNetworkFailure();
        return { roles, profileId };
      }
    } else {
      pendingAuthByProfile.set(profileId, { assertion, roles });
      resetConsecutiveRefreshFailures();
      clearNetworkFailure();
      return { roles, profileId };
    }

    const durationSeconds = Math.min(43200, Math.max(3600, profile.refreshIntervalMinutes * 60));
    const sts = new STSClient(buildStsConfig());
    const result = await sts.send(
      new AssumeRoleWithSAMLCommand({
        RoleArn: roleArn,
        PrincipalArn: principalArn,
        SAMLAssertion: assertion,
        DurationSeconds: durationSeconds,
      })
    );

    const creds = result.Credentials;
    if (!creds?.AccessKeyId || !creds.SecretAccessKey || !creds.SessionToken) {
      noteAuthFailure({
        profileId,
        error: 'STS did not return credentials',
        source: 'refreshProfile',
      });
      return { success: false, error: 'STS did not return credentials' };
    }

    const expiration = creds.Expiration?.toISOString?.() ?? new Date(Date.now() + durationSeconds * 1000).toISOString();
    const sectionName = (profile.credentialProfileName || profile.name || 'default').trim() || 'default';
    writeCredentialsForProfile(sectionName, {
      aws_access_key_id: creds.AccessKeyId,
      aws_secret_access_key: creds.SecretAccessKey,
      aws_session_token: creds.SessionToken,
      region: REGION,
      output: 'json',
    });

    const updated = { ...profile, roleArn, principalArn, expiration };
    saveProfile(updated);
    resetConsecutiveRefreshFailures();
    clearNetworkFailure();
    sendCredentialsRefreshed(profileId);
    notify('Credentials refreshed', `Profile "${profile.name}" has been refreshed.`);
    return { success: true };
  } catch (err: unknown) {
    const message = err instanceof Error ? err.message : String(err);
    noteAuthFailure({ profileId, error: message, errorObject: err, source: 'refreshProfile' });
    return { success: false, error: message };
  }
}

export async function submitCredentials(
  profileId: string,
  username: string,
  password: string
): Promise<RefreshResult> {
  // Always pass credentials through; never persist for useDefaultCredentials so we re-prompt when default has no password.
  return refreshProfile(profileId, { username, password });
}

export async function selectRole(profileId: string, roleIndex: number): Promise<RefreshResult> {
  const pending = pendingAuthByProfile.get(profileId);
  if (!pending) {
    noteAuthFailure({
      profileId,
      error: 'No pending role selection. Please refresh again.',
      source: 'selectRole',
      notifyCredentialsExpired: false,
    });
    return { success: false, error: 'No pending role selection. Please refresh again.' };
  }
  const { assertion, roles } = pending;
  pendingAuthByProfile.delete(profileId);

  if (roleIndex < 0 || roleIndex >= roles.length) {
    noteAuthFailure({
      profileId,
      error: 'Invalid role index',
      source: 'selectRole',
      notifyCredentialsExpired: false,
    });
    return { success: false, error: 'Invalid role index' };
  }
  const role = roles[roleIndex];
  const profile = getProfileById(profileId);
  if (!profile) {
    noteAuthFailure({
      profileId,
      error: 'Profile not found',
      source: 'selectRole',
      notifyCredentialsExpired: false,
    });
    return { success: false, error: 'Profile not found' };
  }

  try {
    const durationSeconds = Math.min(43200, Math.max(3600, profile.refreshIntervalMinutes * 60));
    const sts = new STSClient(buildStsConfig());
    const result = await sts.send(
      new AssumeRoleWithSAMLCommand({
        RoleArn: role.roleArn,
        PrincipalArn: role.principalArn,
        SAMLAssertion: assertion,
        DurationSeconds: durationSeconds,
      })
    );

    const creds = result.Credentials;
    if (!creds?.AccessKeyId || !creds.SecretAccessKey || !creds.SessionToken) {
      noteAuthFailure({
        profileId,
        error: 'STS did not return credentials',
        source: 'selectRole',
      });
      return { success: false, error: 'STS did not return credentials' };
    }

    const expiration = creds.Expiration?.toISOString?.() ?? new Date(Date.now() + durationSeconds * 1000).toISOString();
    const sectionName = (profile.credentialProfileName || profile.name || 'default').trim() || 'default';
    writeCredentialsForProfile(sectionName, {
      aws_access_key_id: creds.AccessKeyId,
      aws_secret_access_key: creds.SecretAccessKey,
      aws_session_token: creds.SessionToken,
      region: REGION,
      output: 'json',
    });

    const updated = {
      ...profile,
      roleArn: role.roleArn,
      principalArn: role.principalArn,
      expiration,
    };
    saveProfile(updated);
    resetConsecutiveRefreshFailures();
    clearNetworkFailure();
    sendCredentialsRefreshed(profileId);
    notify('Credentials refreshed', `Profile "${profile.name}" has been refreshed.`);
    return { success: true };
  } catch (err: unknown) {
    const message = err instanceof Error ? err.message : String(err);
    noteAuthFailure({ profileId, error: message, errorObject: err, source: 'selectRole' });
    return { success: false, error: message };
  }
}

/**
 * Called when user chooses "Refresh all" from the tray. Splits profiles by whether we have
 * credentials (stored default). If any need a prompt, show one modal and use that cred for all;
 * otherwise refresh all with stored creds.
 */
export async function refreshAllProfiles(): Promise<void> {
  const profiles = getProfiles().filter((p) => p.idpEntryUrl?.trim());
  const { needCreds, haveCreds } = await splitProfilesByCredentials(profiles);
  if (needCreds.length > 0) {
    sendRefreshAllRequired(needCreds, haveCreds);
    return;
  }
  for (const id of haveCreds) {
    try {
      await refreshProfile(id);
    } catch {
      // per-profile errors are handled in refreshProfile
    }
  }
}

/** Same threshold as refreshScheduler: refresh when cred expires within this many minutes. */
const REFRESH_THRESHOLD_MINUTES = 15;

function shouldRefreshByExpiration(expiration: string | undefined): boolean {
  if (!expiration) return true;
  const exp = new Date(expiration).getTime();
  const threshold = Date.now() + REFRESH_THRESHOLD_MINUTES * 60 * 1000;
  return exp <= threshold;
}

/**
 * Refreshes only profiles that have autoRefresh enabled and are expired or expiring soon (e.g. after unlock).
 * Splits by whether we have credentials; refreshes those with stored creds first, then one prompt for the rest.
 */
export async function refreshAutoRefreshProfiles(): Promise<void> {
  const allAutoRefresh = getProfiles().filter((p) => p.autoRefresh && p.idpEntryUrl?.trim());
  const profiles = allAutoRefresh.filter((p) => shouldRefreshByExpiration(p.expiration));
  if (profiles.length === 0) return;

  const { needCreds, haveCreds } = await splitProfilesByCredentials(profiles);

  // Refresh profiles we have credentials for (no prompt)
  for (const id of haveCreds) {
    try {
      await refreshProfile(id);
    } catch {
      // per-profile errors are handled in refreshProfile
    }
  }

  // If any need credentials, prompt once; haveCreds already refreshed so pass []
  if (needCreds.length > 0) {
    sendRefreshAllRequired(needCreds, []);
  }
}

/**
 * Refreshes the given profiles: first all credentialProfileIds with the supplied credentials,
 * then all defaultProfileIds using their stored default credentials.
 */
export async function submitCredentialsForRefreshAll(
  credentialProfileIds: string[],
  defaultProfileIds: string[],
  username: string,
  password: string
): Promise<void> {
  const creds = { username, password };
  for (const id of credentialProfileIds) {
    try {
      await refreshProfile(id, creds);
    } catch {
      // per-profile errors are handled in refreshProfile / sendCredentialsExpired
    }
  }
  for (const id of defaultProfileIds) {
    try {
      await refreshProfile(id);
    } catch {
      // per-profile errors are handled in refreshProfile
    }
  }
}
