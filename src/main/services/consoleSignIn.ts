import https from 'https';
import { BrowserWindow } from 'electron';
import { getEnterpriseHttpsAgent, getHttpUserAgent } from './enterpriseTls';
import { getWindowIconPath } from './appIcon';
import { openInBrowser } from './externalBrowser';
import { readCredentialsFile } from './credentialsFile';
import { getProfileById } from './profileStorage';
import { getSettings, saveSettings } from './settingsService';
import { refreshProfile } from './awsAuthService';
import type { Profile } from '../../shared/types';

/**
 * One-click AWS console sign-in for a profile.
 *
 * Uses AWS's documented federation endpoint: exchange a set of temporary credentials for a
 * SigninToken, then build a console login URL from it. Deliberately identical for SAML and
 * Identity Center profiles — by the time credentials reach ~/.aws/credentials the two are
 * indistinguishable, so there is no Entra round-trip and no dependency on which identity the
 * user's browser happens to be signed into.
 *
 * Security: the SigninToken is a bearer credential that grants console access as the role. It is
 * built and consumed entirely in main, is never sent to the renderer, and must never be logged.
 * The renderer only ever learns success or a non-sensitive error message.
 */

/**
 * Regional signin host, not the global `signin.aws.amazon.com`.
 *
 * Both mint sign-in tokens, but only the regional host is accepted as a `redirect_uri` by the
 * multi-session opt-in endpoint — the global one is rejected with HTTP 400. Using it everywhere
 * keeps token minting and the login URL on the same host.
 */
function federationHost(region: string): string {
  return `${region}.signin.aws.amazon.com`;
}
/** Refresh rather than federate when credentials are this close to expiring. */
const MIN_REMAINING_MS = 2 * 60_000;

export type ConsoleSignInResult = { success: true } | { success: false; error: string };

function httpsGetJson(url: string): Promise<{ status: number; body: string }> {
  return new Promise((resolve, reject) => {
    const req = https.get(
      url,
      {
        agent: getEnterpriseHttpsAgent() ?? undefined,
        headers: { accept: 'application/json', 'user-agent': getHttpUserAgent() },
      },
      (res) => {
        const chunks: Buffer[] = [];
        res.on('data', (d: Buffer) => chunks.push(d));
        res.on('end', () => resolve({ status: res.statusCode ?? 0, body: Buffer.concat(chunks).toString('utf8') }));
      }
    );
    req.on('error', reject);
  });
}

/** Region for the console landing page: the profile's SSO region, else what we wrote to the file. */
function regionFor(profile: Profile, section: { region?: string } | undefined): string {
  return (profile.ssoRegion || section?.region || 'us-west-2').trim();
}

function sectionNameOf(profile: Profile): string {
  return (profile.credentialProfileName || profile.name || '').trim();
}

/**
 * Console landing page.
 *
 * Deliberately the SHARED regional host, not the account-scoped multi-session subdomain
 * (`<accountId>.<region>.console.aws.amazon.com`). That subdomain exists and serves in a browser,
 * but the federation endpoint rejects it as a Destination with HTTP 400 — you cannot federate
 * straight into a session slot. AWS assigns the subdomain itself once multi-session is on.
 */
function consoleDestination(region: string): string {
  return `https://${region}.console.aws.amazon.com/console/home?region=${encodeURIComponent(region)}`;
}

/**
 * Open the AWS console for a profile.
 *
 * `interactive` is passed through to the refresh path, so an Identity Center profile whose SSO
 * session has lapsed can prompt for sign-in — this is always a direct user action.
 */
export async function openConsoleForProfile(profileId: string): Promise<ConsoleSignInResult> {
  const profile = getProfileById(profileId);
  if (!profile) return { success: false, error: 'Profile not found.' };

  const sectionName = sectionNameOf(profile);
  if (!sectionName) return { success: false, error: 'Profile has no credentials section name.' };

  // Federating with expired credentials just yields an error from AWS, so refresh first.
  const expiresAt = profile.expiration ? new Date(profile.expiration).getTime() : 0;
  const needsRefresh = !expiresAt || expiresAt - Date.now() < MIN_REMAINING_MS;
  if (needsRefresh) {
    const result = await refreshProfile(profileId, undefined, { interactive: true });
    if ('ssoLoginRequired' in result) {
      return { success: false, error: 'Sign in to AWS first, then try again.' };
    }
    if ('required' in result) {
      return { success: false, error: 'Credentials are required for this profile. Refresh it first.' };
    }
    if ('success' in result && result.success === false) {
      return { success: false, error: result.error };
    }
  }

  let section: { aws_access_key_id?: string; aws_secret_access_key?: string; aws_session_token?: string; region?: string } | undefined;
  try {
    section = readCredentialsFile()[sectionName];
  } catch {
    return { success: false, error: 'Could not read the AWS credentials file.' };
  }
  if (!section?.aws_access_key_id || !section.aws_secret_access_key || !section.aws_session_token) {
    return {
      success: false,
      error: `No temporary credentials found under "${sectionName}". Refresh the profile and try again.`,
    };
  }

  const region = regionFor(profile, section);

  // Note: no SessionDuration parameter. It is not valid with AssumeRole-derived credentials; the
  // console session simply inherits the lifetime of the credentials themselves.
  const session = JSON.stringify({
    sessionId: section.aws_access_key_id,
    sessionKey: section.aws_secret_access_key,
    sessionToken: section.aws_session_token,
  });

  let signinToken: string;
  try {
    const url = `https://${federationHost(region)}/federation?Action=getSigninToken&Session=${encodeURIComponent(session)}`;
    const res = await httpsGetJson(url);
    if (res.status !== 200) {
      // Body can echo credential material; never surface or log it.
      return { success: false, error: `AWS rejected the sign-in request (HTTP ${res.status}).` };
    }
    const parsed = JSON.parse(res.body) as { SigninToken?: string };
    if (!parsed.SigninToken) return { success: false, error: 'AWS did not return a sign-in token.' };
    signinToken = parsed.SigninToken;
  } catch (err) {
    const message = err instanceof Error ? err.message : String(err);
    return { success: false, error: `Could not reach AWS sign-in: ${message}` };
  }

  const destination = consoleDestination(region);
  const loginUrl =
    `https://${federationHost(region)}/federation?Action=login` +
    `&Issuer=${encodeURIComponent('AWSProfileManager')}` +
    `&Destination=${encodeURIComponent(destination)}` +
    `&SigninToken=${encodeURIComponent(signinToken)}`;

  /**
   * Multi-session opt-in runs ONCE, not on every sign-in.
   *
   * Multi-session is what lets several accounts be signed in at the same browser; without it the
   * second profile is refused with "You must first log out before logging into a different AWS
   * account". But calling /sessions/v1/opt-in again when the browser is already opted in with a
   * live session returns HTTP 400 — so chaining it every time breaks the second profile, which is
   * exactly the case it exists to fix.
   *
   * Running it on first use keeps this invisible for a fresh browser. The redirect_uri must be on
   * the regional signin host; the global one returns HTTP 400.
   */
  const settings = getSettings();
  let url = loginUrl;
  if (!settings.consoleMultiSessionOptInDone) {
    url = `https://${federationHost(region)}/sessions/v1/opt-in?redirect_uri=${encodeURIComponent(loginUrl)}`;
    saveSettings({ ...settings, consoleMultiSessionOptInDone: true });
  }

  openConsoleUrl(profile, url);
  return { success: true };
}

/**
 * The AWS console allows one session per browser profile, so opening account B in the default
 * browser silently signs you out of account A. An in-app window on a per-profile partition keeps
 * each account's console session independent, which matters for a multi-account tool.
 *
 * settings.consoleBrowserMode = 'external' falls back to the default browser.
 */
function openConsoleUrl(profile: Profile, loginUrl: string): void {
  const mode = getSettings().consoleBrowserMode ?? 'external';

  if (mode === 'external') {
    openInBrowser(loginUrl, { browserKey: getSettings().consoleBrowser });
    return;
  }

  const iconPath = getWindowIconPath();
  const win = new BrowserWindow({
    width: 1400,
    height: 900,
    autoHideMenuBar: true,
    title: `AWS Console — ${profile.name}`,
    ...(iconPath && { icon: iconPath }),
    webPreferences: {
      partition: `persist:console-${profile.id}`,
      nodeIntegration: false,
      contextIsolation: true,
    },
  });
  void win.loadURL(loginUrl);
}
