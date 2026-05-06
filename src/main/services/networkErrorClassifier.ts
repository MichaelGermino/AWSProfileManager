export type AuthFailureKind = 'network' | 'auth';

const NETWORK_ERROR_CODES = new Set([
  'ENOTFOUND',
  'EAI_AGAIN',
  'ETIMEDOUT',
  'ECONNREFUSED',
  'ECONNRESET',
  'ENETUNREACH',
  'ENETDOWN',
  'EHOSTUNREACH',
  'ERR_NETWORK',
]);

const NETWORK_MESSAGE_REGEX =
  /(getaddrinfo|ENOTFOUND|EAI_AGAIN|ETIMEDOUT|ECONNREFUSED|ECONNRESET|ENETUNREACH|ENETDOWN|EHOSTUNREACH|socket hang up|network error|network is unreachable|dns lookup failed)/i;

interface ErrorLike {
  code?: unknown;
  errno?: unknown;
  message?: unknown;
  cause?: unknown;
  response?: unknown;
  isAxiosError?: unknown;
}

function checkSingleError(err: ErrorLike): boolean {
  if (typeof err.code === 'string' && NETWORK_ERROR_CODES.has(err.code)) return true;
  if (typeof err.errno === 'string' && NETWORK_ERROR_CODES.has(err.errno)) return true;
  // Axios sets isAxiosError=true and exposes response when the server replied. No response + axios error == network failure.
  if (err.isAxiosError === true && err.response == null) return true;
  return false;
}

/**
 * Returns 'network' for transport-layer failures (DNS, offline, refused, timeout) that never reached the IdP.
 * Returns 'auth' for everything else (HTTP 4xx/5xx, bad SAML response, bad credentials, STS errors with a real response).
 *
 * Network failures must NOT count toward the consecutive-failure auto-pause counter, since they cannot
 * cause an account lockout — no login attempt was actually made.
 */
export function classifyAuthFailure(err: unknown): AuthFailureKind {
  if (err == null) return 'auth';

  // Walk err and err.cause chain (axios wraps the original Node error)
  let current: unknown = err;
  for (let depth = 0; depth < 5 && current != null; depth++) {
    if (typeof current === 'object') {
      if (checkSingleError(current as ErrorLike)) return 'network';
      const next = (current as ErrorLike).cause;
      if (next === current) break;
      current = next;
    } else {
      break;
    }
  }

  // Fallback: scan the message text. Some errors are stringified before they reach us.
  const message =
    err instanceof Error
      ? err.message
      : typeof err === 'string'
        ? err
        : typeof (err as ErrorLike).message === 'string'
          ? ((err as ErrorLike).message as string)
          : '';
  if (message && NETWORK_MESSAGE_REGEX.test(message)) return 'network';

  return 'auth';
}
