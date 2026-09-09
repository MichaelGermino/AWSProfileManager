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

  const minted = await mintSigninToken(region, session);
  if ('error' in minted) return { success: false, error: minted.error };
  const loginUrl = buildLoginUrl(region, minted.token);

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
  if (!settings.consoleMultiSessionOptInDone) {
    const optInUrl = `https://${federationHost(region)}/sessions/v1/opt-in?redirect_uri=${encodeURIComponent(loginUrl)}`;
    saveSettings({ ...settings, consoleMultiSessionOptInDone: true });
    openConsoleUrl(profile, optInUrl);
    scheduleFirstOpenFallback(profile, region, session);
    return { success: true };
  }

  openConsoleUrl(profile, loginUrl);
  return { success: true };
}

/**
 * Re-run multi-session opt-in on demand.
 *
 * The automatic attempt happens once per install, but whether it is *needed* is browser state the
 * app cannot see. A new machine, a different default browser, or the user clearing cookies all
 * leave the app thinking it is done when the browser has forgotten — and the symptom shows up much
 * later as "You must first log out before logging into a different AWS account" on the second
 * account. Without this the only cure is hand-editing settings.json.
 *
 * Opens the opt-in URL alone: nothing is chained to it, so a 400 from an already-opted-in browser
 * is just an error page rather than a blocked console.
 */
export function openMultiSessionOptIn(): ConsoleSignInResult {
  const region = (getSettings().defaultSsoRegion || 'us-west-2').trim();
  // redirect_uri is allowlisted to AWS signin/console hosts, so send them to the console home.
  const url =
    `https://${federationHost(region)}/sessions/v1/opt-in` +
    `?redirect_uri=${encodeURIComponent(consoleDestination(region))}`;
  openInBrowser(url, { browserKey: getSettings().consoleBrowser });
  return { success: true };
}

/** How long to give the opt-in redirect before assuming it failed and opening the console directly. */
const FIRST_OPEN_FALLBACK_MS = 6_000;

/**
 * Guarantee the console opens on the one attempt that routes through multi-session opt-in.
 *
 * Whether opt-in is needed is a property of the *browser*, which the app cannot observe: the flag
 * above is app-side state describing browser-side state, so a reinstall, a second machine or a
 * different default browser desyncs them. Guess wrong and AWS answers with a bare HTTP 400 and the
 * user is stranded on an error page with no idea that simply clicking again would work.
 *
 * Detection is not available. `redirect_uri` is validated against an allowlist of AWS signin and
 * console hosts — a loopback URL we could serve ourselves is rejected with 400 — so there is no
 * callback to observe and no way to learn the outcome.
 *
 * So don't try to detect: open the console unconditionally a few seconds later. Opt-in worked and
 * the user gets a harmless duplicate tab; it failed and they get a working console beside the
 * error. Either way nobody hits a dead end, and it costs one extra tab exactly once per install.
 */
function scheduleFirstOpenFallback(profile: Profile, region: string, session: string): void {
  // Embedded mode would mean a second WINDOW rather than a background tab, which is worse than the
  // problem. It is also the mode where the user can simply close the window and click again.
  if ((getSettings().consoleBrowserMode ?? 'external') !== 'external') return;

  setTimeout(() => {
    void (async () => {
      // A fresh token, not the one already in loginUrl: sign-in tokens are single-use as far as we
      // know, so reusing that URL would risk the fallback tab failing in the success case.
      const minted = await mintSigninToken(region, session);
      if ('error' in minted) {
        console.warn(`[console] multi-session fallback could not mint a sign-in token: ${minted.error}`);
        return;
      }
      openConsoleUrl(profile, buildLoginUrl(region, minted.token));
    })();
  }, FIRST_OPEN_FALLBACK_MS);
}

function buildLoginUrl(region: string, signinToken: string): string {
  return (
    `https://${federationHost(region)}/federation?Action=login` +
    `&Issuer=${encodeURIComponent('AWSProfileManager')}` +
    `&Destination=${encodeURIComponent(consoleDestination(region))}` +
    `&SigninToken=${encodeURIComponent(signinToken)}`
  );
}

/** `session` is the JSON credential blob; it and the returned token are bearer secrets — never log either. */
async function mintSigninToken(
  region: string,
  session: string
): Promise<{ token: string } | { error: string }> {
  try {
    const url = `https://${federationHost(region)}/federation?Action=getSigninToken&Session=${encodeURIComponent(session)}`;
    const res = await httpsGetJson(url);
    if (res.status !== 200) {
      // Body can echo credential material; never surface or log it.
      return { error: `AWS rejected the sign-in request (HTTP ${res.status}).` };
    }
    const parsed = JSON.parse(res.body) as { SigninToken?: string };
    if (!parsed.SigninToken) return { error: 'AWS did not return a sign-in token.' };
    return { token: parsed.SigninToken };
  } catch (err) {
    const message = err instanceof Error ? err.message : String(err);
    return { error: `Could not reach AWS sign-in: ${message}` };
  }
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
