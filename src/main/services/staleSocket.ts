/**
 * Kept-alive HTTPS sockets (the enterprise agent sets keepAlive) can be closed by the server while
 * idle. The next request that reuses one fails with ECONNRESET/EPIPE before the server has seen
 * anything, so repeating it once is safe — the pattern Node's http docs recommend, guarded on
 * `req.reusedSocket` so a reset on a fresh connection (where the server may have acted) is never
 * retried. Fixed 2026-11-14: 1.x failed such requests (e.g. the console fallback tab, a silent SSO
 * refresh after an idle period).
 */
import type { ClientRequest } from 'http';

const STALE_CODES = new Set(['ECONNRESET', 'EPIPE']);

/** Tag a request error with whether the request went out on a reused socket. */
export function tagReused(req: ClientRequest, err: Error): Error {
  return Object.assign(err, { reusedSocket: req.reusedSocket === true });
}

export function isStaleSocketError(err: unknown): boolean {
  const e = err as NodeJS.ErrnoException & { reusedSocket?: boolean };
  return e?.reusedSocket === true && !!e.code && STALE_CODES.has(e.code);
}

/** Run `once`, and run it one more time if it failed on a stale kept-alive socket. */
export async function withStaleSocketRetry<T>(once: () => Promise<T>): Promise<T> {
  try {
    return await once();
  } catch (err) {
    if (isStaleSocketError(err)) return once();
    throw err;
  }
}
