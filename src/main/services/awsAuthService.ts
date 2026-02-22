import axios, { type AxiosResponse } from 'axios';
import { wrapper } from 'axios-cookiejar-support';
import { CookieJar } from 'tough-cookie';
import * as cheerio from 'cheerio';
import { parseStringPromise } from 'xml2js';
import { AssumeRoleWithSAMLCommand, STSClient } from '@aws-sdk/client-sts';
import { getProfileById, getProfiles, saveProfile } from './profileStorage';
import { getStoredCredentials, setStoredCredentials, DEFAULT_CREDENTIALS_ID } from './credentialStorage';
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

function sendRefreshAllRequired(credentialProfileIds: string[], defaultProfileIds: string[]) {
  if (mainWindowRef && !mainWindowRef.isDestroyed()) {
    mainWindowRef.show();
    mainWindowRef.focus();
  }
  mainWindowRef?.webContents.send('auth:refreshAllRequired', credentialProfileIds, defaultProfileIds);
}

interface PendingAuth {
  assertion: string;
  roles: AwsRole[];
}

const pendingAuthByProfile = new Map<string, PendingAuth>();

const REGION = 'us-west-2';
const SAML_ATTR_ROLE = 'https://aws.amazon.com/SAML/Attributes/Role';

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
  password: string
): Promise<{ assertion: string; roles: AwsRole[] }> {
  const jar = new CookieJar();
  const client = wrapper(
    axios.create({
      jar,
      maxRedirects: 0,
      validateStatus: () => true,
      headers: SESSION_HEADERS,
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
    let stored = await getStoredCredentials(credentialsKey);
    if (!stored?.password && options.profileId) {
      stored = await getStoredCredentials(options.profileId);
    }
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
    const { roles } = await performLogin(idpEntryUrl, username, password);
    setCachedRoles(idpEntryUrl, roles);
    return { roles };
  } catch (err) {
    const message = err instanceof Error ? err.message : String(err);
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
    const { roles } = await performLogin(idpEntryUrl, username, password);
    setCachedRoles(idpEntryUrl, roles);
    return { roles };
  } catch (err) {
    const message = err instanceof Error ? err.message : String(err);
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
  if (!profile) return { success: false, error: 'Profile not found' };
  if (!profile.idpEntryUrl?.trim()) {
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
      return {
        success: false,
        error: `Could not read saved credentials: ${msg}. Try entering credentials again.`,
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
    const { assertion, roles } = await performLogin(profile.idpEntryUrl, username, password);
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
        return { roles, profileId };
      }
    } else {
      pendingAuthByProfile.set(profileId, { assertion, roles });
      return { roles, profileId };
    }

    const durationSeconds = Math.min(43200, Math.max(3600, profile.refreshIntervalMinutes * 60));
    const sts = new STSClient({ region: REGION });
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
    sendCredentialsRefreshed(profileId);
    notify('Credentials refreshed', `Profile "${profile.name}" has been refreshed.`);
    return { success: true };
  } catch (err: unknown) {
    const message = err instanceof Error ? err.message : String(err);
    sendCredentialsExpired(profileId, message);
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
    return { success: false, error: 'No pending role selection. Please refresh again.' };
  }
  const { assertion, roles } = pending;
  pendingAuthByProfile.delete(profileId);

  if (roleIndex < 0 || roleIndex >= roles.length) {
    return { success: false, error: 'Invalid role index' };
  }
  const role = roles[roleIndex];
  const profile = getProfileById(profileId);
  if (!profile) return { success: false, error: 'Profile not found' };

  try {
    const durationSeconds = Math.min(43200, Math.max(3600, profile.refreshIntervalMinutes * 60));
    const sts = new STSClient({ region: REGION });
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
    sendCredentialsRefreshed(profileId);
    notify('Credentials refreshed', `Profile "${profile.name}" has been refreshed.`);
    return { success: true };
  } catch (err: unknown) {
    const message = err instanceof Error ? err.message : String(err);
    return { success: false, error: message };
  }
}

/**
 * Called when user chooses "Refresh all" from the tray. If any profiles need credentials
 * (useDefaultCredentials off), sends a single prompt to the renderer; otherwise refreshes all in sequence.
 */
export async function refreshAllProfiles(): Promise<void> {
  const profiles = getProfiles().filter((p) => p.idpEntryUrl?.trim());
  const needCreds = profiles.filter((p) => !p.useDefaultCredentials).map((p) => p.id);
  const useDefault = profiles.filter((p) => p.useDefaultCredentials).map((p) => p.id);
  if (needCreds.length > 0) {
    sendRefreshAllRequired(needCreds, useDefault);
    return;
  }
  for (const p of profiles) {
    try {
      await refreshProfile(p.id);
    } catch {
      // per-profile errors are handled in refreshProfile
    }
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
