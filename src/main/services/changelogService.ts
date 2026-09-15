import fs from 'fs';
import https from 'https';
import path from 'path';
import { app } from 'electron';
import { isWhatsNewForced } from './devFlags';
import { getEnterpriseHttpsAgent, getHttpUserAgent } from './enterpriseTls';
import { getSettings, saveSettings } from './settingsService';
import { getProfiles } from './profileStorage';

/**
 * "What's new" after an update.
 *
 * The GitHub release for the running version is the single source of truth — release notes are
 * written once, where they are already written, and nothing is duplicated into the repo. No notes
 * on the release means no popup: an empty modal is worse than none.
 *
 * Shown at most once per version, tracked by settings.lastChangelogVersionSeen.
 *
 * The one exception is the dev flag (see devFlags.ts), which replays the modal on every launch and
 * prefers `docs/releasenotes/<version>.md` if that file exists — so notes can be written and seen
 * in the running app before the release they will live on is published. That path never records
 * anything as seen, so it cannot consume the real one-shot.
 */

const GITHUB_OWNER = 'MichaelGermino';
const GITHUB_REPO = 'AWSProfileManager';
const FETCH_TIMEOUT_MS = 10_000;

export interface PendingChangelog {
  version: string;
  /** Raw Markdown, exactly as written on the GitHub release. Rendered in the renderer. */
  notes: string;
  /** Link to the release page, for "View on GitHub". */
  url: string;
}

interface GithubRelease {
  body?: string;
  html_url?: string;
}

/** Release for a version tag, or null for any failure. `null` never marks the version as seen. */
function fetchRelease(version: string): Promise<{ ok: true; release: GithubRelease } | { ok: false; missing: boolean }> {
  // Tags carry the v prefix; the version in package.json does not.
  const path = `/repos/${GITHUB_OWNER}/${GITHUB_REPO}/releases/tags/v${encodeURIComponent(version)}`;
  return new Promise((resolve) => {
    const req = https.get(
      {
        host: 'api.github.com',
        path,
        agent: getEnterpriseHttpsAgent() ?? undefined,
        headers: {
          accept: 'application/vnd.github+json',
          // GitHub rejects requests without one.
          'user-agent': getHttpUserAgent(),
        },
      },
      (res) => {
        const chunks: Buffer[] = [];
        res.on('data', (d: Buffer) => chunks.push(d));
        res.on('end', () => {
          if (res.statusCode !== 200) {
            // 404 means no published release for this tag — a dev build, or a draft. Settled, not
            // a transient failure, so let the caller stop asking.
            resolve({ ok: false, missing: res.statusCode === 404 });
            return;
          }
          try {
            resolve({ ok: true, release: JSON.parse(Buffer.concat(chunks).toString('utf8')) as GithubRelease });
          } catch {
            resolve({ ok: false, missing: false });
          }
        });
      }
    );
    req.setTimeout(FETCH_TIMEOUT_MS, () => req.destroy());
    req.on('error', () => resolve({ ok: false, missing: false }));
  });
}

/**
 * Notes checked into the repo for this version, or null.
 *
 * Only reachable in a dev tree: `docs/` is not packaged, so in an installed build this always
 * misses and the GitHub fetch below is the only source — exactly as it ships.
 */
function readLocalNotes(version: string): string | null {
  // app.getAppPath() rather than a count of `..` from __dirname: this file sits in main/services/,
  // one level deeper than main.ts, and getting that count wrong fails silently by falling through
  // to GitHub. In dev getAppPath() is the repo root; packaged it is inside app.asar, where docs/
  // is not bundled, so this misses and the GitHub fetch below stays the only source.
  const file = path.join(app.getAppPath(), 'docs', 'releasenotes', `${version}.md`);
  try {
    const body = fs.readFileSync(file, 'utf8').trim();
    return body.length > 0 ? body : null;
  } catch {
    return null;
  }
}

/** Record a version as shown, so it never appears again. A no-op under the dev flag. */
export function markChangelogSeen(version: string): void {
  if (isWhatsNewForced()) return;
  const settings = getSettings();
  if (settings.lastChangelogVersionSeen === version) return;
  saveSettings({ ...settings, lastChangelogVersionSeen: version });
}

/**
 * Release notes to show now, or null.
 *
 * Marks the version seen for every *settled* outcome — shown, no notes, no release — so a launch
 * costs at most one API call per version. A network failure is deliberately NOT settled: it stays
 * pending and is retried next launch, because "offline right now" should not silently consume the
 * one chance to show what changed.
 */
export async function getPendingChangelog(): Promise<PendingChangelog | null> {
  const version = app.getVersion();
  const settings = getSettings();
  const forced = isWhatsNewForced();

  if (forced) {
    // Repo notes first so unreleased versions have something to show; GitHub still answers for a
    // version whose .md was never written.
    const local = readLocalNotes(version);
    if (local) {
      return {
        version,
        notes: local,
        url: `https://github.com/${GITHUB_OWNER}/${GITHUB_REPO}/releases/tag/v${version}`,
      };
    }
  } else if (settings.lastChangelogVersionSeen === version) {
    return null;
  }

  /**
   * A first-ever launch is not an update. Nothing recorded, no profiles and setup not finished
   * means this install has never run, and that user should meet the setup wizard rather than
   * release notes for a version they have no history with.
   */
  const looksLikeFreshInstall =
    !settings.lastChangelogVersionSeen && settings.setupCompleted !== true && getProfiles().length === 0;
  if (looksLikeFreshInstall && !forced) {
    markChangelogSeen(version);
    return null;
  }

  const result = await fetchRelease(version);
  if (!result.ok) {
    if (result.missing) markChangelogSeen(version);
    return null;
  }

  const notes = (result.release.body ?? '').trim();
  if (!notes) {
    markChangelogSeen(version);
    return null;
  }

  return {
    version,
    notes,
    url: result.release.html_url ?? `https://github.com/${GITHUB_OWNER}/${GITHUB_REPO}/releases/tag/v${version}`,
  };
}
