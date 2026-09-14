import { app, dialog, net, protocol } from 'electron';
import fs from 'fs';
import path from 'path';
import type { BrowserWindow } from 'electron';
import { getSettings, saveSettings } from './settingsService';

/**
 * User-supplied background video.
 *
 * The file is COPIED into userData rather than referenced where the user picked it. A path into
 * Downloads or a network share would break the moment the file moved, and the background would
 * silently vanish with nothing in the UI to explain why.
 *
 * It is served over a custom `appmedia://` scheme rather than `file://`, because the renderer runs
 * on an http://localhost origin in dev and file:// when packaged. A file:// URL is blocked from the
 * http origin, so a path that worked in a packaged build would fail in dev (and vice versa). One
 * scheme behaves the same in both.
 *
 * `net.fetch` does the actual reading: it honours HTTP range requests, which <video> relies on.
 * Returning the whole file as one Response makes seeking and looping unreliable on larger files.
 */

export const BACKGROUND_SCHEME = 'appmedia';

/** Registered before app ready; `stream` is what allows ranged media responses. */
export const BACKGROUND_SCHEME_PRIVILEGES = {
  scheme: BACKGROUND_SCHEME,
  privileges: { standard: true, secure: true, supportFetchAPI: true, stream: true },
};

const FOLDER_NAME = 'background';
const ALLOWED_EXTENSIONS = ['mp4', 'webm'];

function backgroundDir(): string {
  return path.join(app.getPath('userData'), FOLDER_NAME);
}

/** The stored video if it is still on disk, else null (the file can be deleted behind our back). */
export function getBackgroundVideoPath(): string | null {
  try {
    const stored = getSettings().backgroundVideoPath;
    if (!stored) return null;
    return fs.existsSync(stored) ? stored : null;
  } catch {
    return null;
  }
}

/**
 * Serve the current video. The request path is ignored — it carries only a cache-busting token so
 * that replacing the video produces a URL the renderer treats as new.
 */
export function registerBackgroundProtocol(): void {
  protocol.handle(BACKGROUND_SCHEME, async () => {
    const filePath = getBackgroundVideoPath();
    if (!filePath) return new Response(null, { status: 404 });
    try {
      return await net.fetch(`file://${filePath.replace(/\\/g, '/')}`);
    } catch {
      return new Response(null, { status: 404 });
    }
  });
}

export type ChooseVideoResult =
  | { canceled: true }
  | { success: true; fileName: string; updatedAt: number }
  | { success: false; error: string };

/** Pick a video, copy it into userData, and record it in settings. Replaces any previous one. */
export async function chooseBackgroundVideo(win: BrowserWindow | null): Promise<ChooseVideoResult> {
  const options = {
    title: 'Choose a background video',
    properties: ['openFile'] as ('openFile')[],
    filters: [{ name: 'Video', extensions: ALLOWED_EXTENSIONS }],
  };
  const result = win
    ? await dialog.showOpenDialog(win, options)
    : await dialog.showOpenDialog(options);
  if (result.canceled || !result.filePaths?.length) return { canceled: true };

  const source = result.filePaths[0];
  const ext = path.extname(source).replace('.', '').toLowerCase();
  if (!ALLOWED_EXTENSIONS.includes(ext)) {
    return { success: false, error: `Unsupported format ".${ext}". Use ${ALLOWED_EXTENSIONS.join(' or ')}.` };
  }

  try {
    const dir = backgroundDir();
    if (!fs.existsSync(dir)) fs.mkdirSync(dir, { recursive: true });

    // Clear previous files first, so switching mp4 -> webm doesn't leave the old one orphaned.
    for (const existing of fs.readdirSync(dir)) {
      try {
        fs.unlinkSync(path.join(dir, existing));
      } catch {
        // A file held open by the still-playing <video> can refuse to delete; the new name wins
        // anyway, so this is not worth failing the whole operation over.
      }
    }

    const updatedAt = Date.now();
    const destination = path.join(dir, `video-${updatedAt}.${ext}`);
    fs.copyFileSync(source, destination);

    const settings = getSettings();
    saveSettings({
      ...settings,
      backgroundVideoPath: destination,
      backgroundVideoName: path.basename(source),
      backgroundVideoUpdatedAt: updatedAt,
    });

    return { success: true, fileName: path.basename(source), updatedAt };
  } catch (err) {
    return { success: false, error: err instanceof Error ? err.message : String(err) };
  }
}

/** Forget the custom video and delete the copy. The bundled default (if any) takes over again. */
export function clearBackgroundVideo(): void {
  const stored = getSettings().backgroundVideoPath;
  if (stored) {
    try {
      fs.rmSync(stored, { force: true });
    } catch {
      // Best effort — the settings entry below is what actually stops it being used.
    }
  }
  const settings = getSettings();
  saveSettings({
    ...settings,
    backgroundVideoPath: undefined,
    backgroundVideoName: undefined,
    backgroundVideoUpdatedAt: undefined,
  });
}
