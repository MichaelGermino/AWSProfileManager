import { app } from 'electron';
import { isWhatsNewForced } from './devFlags';
import { getSettings, saveSettings } from './settingsService';
import { getProfiles } from './profileStorage';

/**
 * State for the guided feature tour shown after an update.
 *
 * Main owns only the *bookkeeping* — which tours have been seen, whether this install has any
 * history, whether the dev flag is on. The tours themselves live in the renderer
 * (`src/renderer/tour/tourSteps.ts`) because every step is a DOM selector and a route; main has no
 * business knowing about either, and duplicating the ids here would be a third place to keep in
 * sync. So main answers "what has been seen", the renderer answers "what is due".
 */

export interface TourState {
  /** Tour ids already shown. The renderer picks the first of its tours not in this list. */
  seenTourIds: string[];
  /** Dev flag: replay regardless of seenTourIds, and never record what was replayed. */
  forced: boolean;
  /**
   * This install has never been used: setup not finished and no profiles. A first-ever launch is
   * not an update — that user meets the setup wizard, not a tour of features they have no history
   * with. Deliberately does NOT consult `lastChangelogVersionSeen` (which changelogService writes
   * on the same launch) so the two checks cannot race.
   */
  freshInstall: boolean;
  version: string;
}

export function getTourState(): TourState {
  const settings = getSettings();
  return {
    seenTourIds: Array.isArray(settings.seenTourIds) ? settings.seenTourIds : [],
    forced: isWhatsNewForced(),
    freshInstall: settings.setupCompleted !== true && getProfiles().length === 0,
    version: app.getVersion(),
  };
}

/** Record a tour as shown, so it never appears again. A no-op under the dev flag. */
export function markTourSeen(id: string): void {
  if (isWhatsNewForced()) return;
  const settings = getSettings();
  const seen = Array.isArray(settings.seenTourIds) ? settings.seenTourIds : [];
  if (seen.includes(id)) return;
  saveSettings({ ...settings, seenTourIds: [...seen, id] });
}

/**
 * Forget every tour and the release-notes marker, so the next launch takes the real first-run-after-
 * update path. This is what the dev flag cannot test: the flag bypasses the trigger, this exercises
 * it. Exposed from Settings → Advanced → Developer options.
 */
export function resetWhatsNewState(): void {
  const settings = getSettings();
  const { lastChangelogVersionSeen: _seen, seenTourIds: _tours, ...rest } = settings;
  saveSettings(rest);
}
