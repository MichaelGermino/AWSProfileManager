import https from 'https';
import { app } from 'electron';
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

/** Record a version as shown, so it never appears again. */
export function markChangelogSeen(version: string): void {
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
  if (settings.lastChangelogVersionSeen === version) return null;

  /**
   * A first-ever launch is not an update. Nothing recorded, no profiles and setup not finished
   * means this install has never run, and that user should meet the setup wizard rather than
   * release notes for a version they have no history with.
   */
  const looksLikeFreshInstall =
    !settings.lastChangelogVersionSeen && settings.setupCompleted !== true && getProfiles().length === 0;
  if (looksLikeFreshInstall) {
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
