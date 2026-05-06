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
 *     proxies introduce while they perform their own auth.
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
 */

import * as https from 'https';
import * as tls from 'tls';
import { Agent, setGlobalDispatcher } from 'undici';

let installed = false;
let cachedAgent: https.Agent | null = null;
let cachedCombinedCAs: string[] | null = null;

interface TlsWithCAs {
  getCACertificates?: (type: 'system' | 'bundled' | 'extra') => string[];
  rootCertificates?: readonly string[];
}

export function applyEnterpriseTls(): void {
  if (installed) return;
  installed = true;

  const tlsAny = tls as unknown as TlsWithCAs;
  if (typeof tlsAny.getCACertificates !== 'function') {
    console.warn('[enterprise-tls] skipped: tls.getCACertificates not available (need Node 22.15+ / Electron 33+)');
    return;
  }

  let systemCAs: string[];
  try {
    systemCAs = tlsAny.getCACertificates('system');
  } catch (err) {
    console.warn('[enterprise-tls] failed to read OS trust store:', err);
    return;
  }

  if (!systemCAs || systemCAs.length === 0) {
    console.log('[enterprise-tls] OS trust store returned 0 certs; nothing to install');
    return;
  }

  const rootCAs = tlsAny.rootCertificates ?? [];
  const combinedCAs = [...systemCAs, ...rootCAs];
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

  // undici / global fetch(): 120s connect timeout for slow corporate proxies.
  try {
    setGlobalDispatcher(
      new Agent({
        connect: {
          ca: combinedCAs,
          timeout: 120_000,
        },
      })
    );
  } catch (err) {
    console.warn('[enterprise-tls] undici dispatcher setup failed:', err);
  }

  console.log(`[enterprise-tls] installed (${systemCAs.length} OS CA(s) added to TLS trust)`);
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
