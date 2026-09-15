/**
 * Developer switches for the post-update "what's new" surfaces.
 *
 * The release-notes modal and the guided feature tour both show at most once per install, which
 * makes them nearly impossible to iterate on: you get one shot per version, on a machine that has
 * already taken it. This flag replays both on every launch without touching the stored state, so
 * the real trigger path stays exactly as it ships.
 *
 * Two ways to set it, because neither covers both cases:
 *   - `FORCE_WHATS_NEW=1 npm run dev` — the dev loop. dotenv also reads it from a root `.env`.
 *   - `AWSProfileManager.exe --force-whats-new` — a packaged build, where there is no npm script.
 *
 * It is deliberately NOT gated on `app.isPackaged`: verifying this against the installer is the
 * whole point, and the flag only replays UI that every user sees anyway — it reveals nothing and
 * writes nothing.
 */

const ENV_KEYS = ['FORCE_WHATS_NEW', 'FORCE_TOUR'];
const ARGV_FLAGS = ['--force-whats-new', '--force-tour'];

/**
 * True when release notes and the feature tour should be shown regardless of what has already been
 * seen. Read live rather than cached at import time so a test can set the env var late.
 */
export function isWhatsNewForced(): boolean {
  if (ENV_KEYS.some((key) => process.env[key] === '1')) return true;
  return process.argv.some((arg) => ARGV_FLAGS.includes(arg));
}
