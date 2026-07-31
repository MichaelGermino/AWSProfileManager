/**
 * Enterprise / TLS-inspecting-proxy support.
 *
 * On corporate machines that run a TLS-inspection proxy (Zscaler, Netskope, Websense,
 * Palo Alto, Cisco Umbrella, etc.), outbound HTTPS is intercepted and re-signed by a
 * proxy CA that the enterprise pre-provisions into the OS trust store. Node ships its
 * own bundled Mozilla CA list and does NOT consult the OS store, so every fetch fails
 * with UNABLE_TO_GET_ISSUER_CERT_LOCALLY / SELF_SIGNED_CERT_IN_CHAIN until we extend
 * Node's trust to include OS-provisioned CAs.
 *
 * Strategy: read the OS trust store via tls.getCACertificates('system') (Node 22.15+),
 * combine it with the bundled Mozilla roots, and apply that trust three ways:
 *
 *  1. https.globalAgent.options.ca — covers axios (no custom agent), electron-updater,
 *     and any other code path that uses Node's default HTTPS agent.
 *
 *  2. Undici global dispatcher with 120s connect timeout — covers global fetch()
 *     (used by aiService.ts). The 120s tolerates the slow first-handshake corporate
 *     proxies introduce while they perform their own auth. This dispatcher also
 *     rewrites the User-Agent (see below).
 *
 *  3. An exported https.Agent that callers who construct their OWN agents can plug in.
 *     The AWS SDK's @smithy/node-http-handler creates a fresh https.Agent that does
 *     not consult globalAgent, so STSClient must be constructed with the exported
 *     agent or it will fail TLS verification behind a corporate proxy.
 *
 * NOTE on tls.createSecureContext: in Node 22.15 (Electron 41) it's exported as a
 * non-configurable getter, so the win-ca-style "patch every TLS context" approach
 * doesn't work. Hence the per-caller approach above. If a future caller is added
 * that uses neither the global agent, undici, nor the exported agent, it will need
 * its own enterprise-TLS wiring.
 *
 * Renderer-process HTTPS is unaffected; Chromium's network stack already trusts the
 * OS store. So is BrowserWindow.loadURL (browserFetchService.ts).
 *
 * Do NOT replace this with NODE_TLS_REJECT_UNAUTHORIZED=0 or rejectUnauthorized:false.
 * Verification stays on; we only extend trust to OS-provisioned roots.
 *
 * ---------------------------------------------------------------------------
 * User-Agent rewriting (also handled by the undici dispatcher)
 *
 * Node's fetch() sends a literal `User-Agent: node` when the caller doesn't set one.
 * Some API edges/WAFs treat that as a bot signature and return a bare HTML 403 before
 * the request reaches the application — which is exactly what poppy.ca.gov does to
 * GET /api/models. The symptom is baffling because the key and URL are both correct.
 *
 * The dispatcher below composes an interceptor that swaps the default `node` UA for a
 * real product token. It replaces ONLY the exact value `node`, so a caller that sets
 * its own User-Agent still wins. Note the interceptor must *replace*, not fill-in:
 * fetch() has already applied its default by the time dispatch() is reached, so an
 * "only if absent" guard would never fire.
 */

import * as https from 'https';
import * as tls from 'tls';
import { app } from 'electron';
import { Agent, setGlobalDispatcher } from 'undici';

let installed = false;
let cachedAgent: https.Agent | null = null;
let cachedCombinedCAs: string[] | null = null;
let cachedUserAgent: string | null = null;

/** Node's built-in default User-Agent, and the only value we will overwrite. */
const NODE_DEFAULT_UA = 'node';

/**
 * Product token sent on outbound HTTPS from main, e.g. `AWSProfileManager/1.3.0`.
 * Resolved lazily: `app` is unavailable if this module is ever loaded outside Electron.
 */
export function getHttpUserAgent(): string {
  if (cachedUserAgent) return cachedUserAgent;
  let version = '0.0.0';
  try {
    version = app.getVersion();
  } catch {
    // not running under Electron (or app not initialised) — fall back to a valid token
  }
  cachedUserAgent = `AWSProfileManager/${version}`;
  return cachedUserAgent;
}

interface TlsWithCAs {
  getCACertificates?: (type: 'system' | 'bundled' | 'extra') => string[];
  rootCertificates?: readonly string[];
}

/** Reads the OS trust store and merges it with Node's bundled roots. Null if unavailable. */
function readCombinedCAs(): string[] | null {
  const tlsAny = tls as unknown as TlsWithCAs;
  if (typeof tlsAny.getCACertificates !== 'function') {
    console.warn('[enterprise-tls] skipped: tls.getCACertificates not available (need Node 22.15+ / Electron 33+)');
    return null;
  }

  let systemCAs: string[];
  try {
    systemCAs = tlsAny.getCACertificates('system');
  } catch (err) {
    console.warn('[enterprise-tls] failed to read OS trust store:', err);
    return null;
  }

  if (!systemCAs || systemCAs.length === 0) {
    console.log('[enterprise-tls] OS trust store returned 0 certs; nothing to install');
    return null;
  }

  const rootCAs = tlsAny.rootCertificates ?? [];
  return [...systemCAs, ...rootCAs];
}

/**
 * Installs the undici global dispatcher used by global fetch(). Runs even when no OS
 * CAs were found, because it also carries the User-Agent rewrite that keeps requests
 * from being 403'd by bot filters — that fix must not be contingent on TLS setup.
 */
function installUndiciDispatcher(combinedCAs: string[] | null): void {
  try {
    const base = new Agent({
      connect: {
        ...(combinedCAs ? { ca: combinedCAs } : {}),
        timeout: 120_000,
      },
    });

    const userAgent = getHttpUserAgent();
    const withUserAgent = base.compose((dispatch) => (opts, handler) => {
      const headers = opts.headers;
      // fetch() normalises to an object here, but dispatch() also accepts a flat
      // [k, v, k, v] array — handle both so this can't silently stop working.
      if (Array.isArray(headers)) {
        for (let i = 0; i < headers.length - 1; i += 2) {
          if (String(headers[i]).toLowerCase() === 'user-agent' && headers[i + 1] === NODE_DEFAULT_UA) {
            headers[i + 1] = userAgent;
          }
        }
      } else if (headers && typeof headers === 'object') {
        const bag = headers as Record<string, unknown>;
        for (const key of Object.keys(bag)) {
          if (key.toLowerCase() === 'user-agent' && bag[key] === NODE_DEFAULT_UA) {
            bag[key] = userAgent;
          }
        }
      }
      return dispatch(opts, handler);
    });

    setGlobalDispatcher(withUserAgent);
  } catch (err) {
    console.warn('[enterprise-tls] undici dispatcher setup failed:', err);
  }
}

export function applyEnterpriseTls(): void {
  if (installed) return;
  installed = true;

  const combinedCAs = readCombinedCAs();

  if (combinedCAs) {
    cachedCombinedCAs = combinedCAs;
    cachedAgent = new https.Agent({ ca: combinedCAs, keepAlive: true });

    // Default https.Agent: covers axios (no custom httpsAgent), electron-updater,
    // and anything else that uses Node's default agent.
    try {
      const ga = https.globalAgent as unknown as { options?: Record<string, unknown> };
      if (ga.options) {
        ga.options.ca = combinedCAs;
      } else {
        ga.options = { ca: combinedCAs };
      }
    } catch (err) {
      console.warn('[enterprise-tls] could not extend https.globalAgent CA:', err);
    }
  }

  // undici / global fetch(): 120s connect timeout + User-Agent rewrite. Always installed.
  installUndiciDispatcher(combinedCAs);

  const caCount = combinedCAs ? combinedCAs.length : 0;
  console.log(
    `[enterprise-tls] installed (${caCount} CA(s) in TLS trust; User-Agent ${getHttpUserAgent()})`
  );
}

/** Returns an https.Agent configured with OS-trusted CAs, or null if enterprise TLS
 *  isn't active (no system CAs found, older Node, etc.). Callers that construct their
 *  own agent — notably the AWS SDK via @smithy/node-http-handler — must pass this in
 *  or they will fail TLS verification behind a corporate proxy. */
export function getEnterpriseHttpsAgent(): https.Agent | null {
  return cachedAgent;
}

/** Returns the combined OS + bundled-root CA list, or null if enterprise TLS isn't active. */
export function getEnterpriseCombinedCAs(): string[] | null {
  return cachedCombinedCAs;
}
