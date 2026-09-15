import { app, dialog, protocol } from 'electron';
import fs from 'fs';
import path from 'path';
import { Readable } from 'stream';
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
 * RANGE REQUESTS ARE LOAD-BEARING — see `registerBackgroundProtocol`.
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

const MIME_BY_EXTENSION: Record<string, string> = { mp4: 'video/mp4', webm: 'video/webm' };

/** A file, or a slice of one, as the web ReadableStream a Response wants. */
function fileStream(filePath: string, start?: number, end?: number): ReadableStream<Uint8Array> {
  // `end` is inclusive for both createReadStream and HTTP ranges, so it passes through unchanged.
  return Readable.toWeb(fs.createReadStream(filePath, { start, end })) as ReadableStream<Uint8Array>;
}

type ParsedRange = { start: number; end: number } | 'unsatisfiable' | null;

/** A single `bytes=` range. Multi-range requests are not parsed; browsers do not send them for media. */
function parseRange(header: string | null, size: number): ParsedRange {
  if (!header) return null;
  const match = /^bytes=(\d*)-(\d*)$/.exec(header.trim());
  if (!match) return null;
  const [, rawStart, rawEnd] = match;
  if (rawStart === '' && rawEnd === '') return null;

  let start: number;
  let end: number;
  if (rawStart === '') {
    // Suffix form (`bytes=-N`): the last N bytes. Chromium uses this to read an MP4's trailing
    // index when `moov` is at the end of the file.
    const suffixLength = Number(rawEnd);
    if (!Number.isFinite(suffixLength) || suffixLength <= 0) return 'unsatisfiable';
    start = Math.max(0, size - suffixLength);
    end = size - 1;
  } else {
    start = Number(rawStart);
    end = rawEnd === '' ? size - 1 : Math.min(Number(rawEnd), size - 1);
  }
  if (!Number.isFinite(start) || !Number.isFinite(end) || start >= size || end < start) {
    return 'unsatisfiable';
  }
  return { start, end };
}

/**
 * Serve the current video, honouring HTTP range requests.
 *
 * The request path is ignored — it carries only a cache-busting token so that replacing the video
 * produces a URL the renderer treats as new.
 *
 * **Answering every request with the whole file from byte 0 is not a slower correct answer, it is a
 * wrong one.** Chromium asks for byte ranges while playing; a `200` starting at 0 in reply to
 * `Range: bytes=N-` hands it bytes it will file at offset N, and it also tells Chromium the source
 * is not seekable. For a *fragmented* MP4 — anything saved from a DASH stream — that is fatal: the
 * real duration lives in the `sidx`/`moof` index rather than in `mvhd` (which such files leave at
 * 0), and reading that index requires seeking. Without it the demuxer hits end-of-stream at
 * whatever fragment it managed to parse, and `loop` turns that into a restart partway through —
 * the video appearing to loop early. A plain progressive MP4 hides the bug by being readable
 * front-to-back.
 *
 * So: real `206` replies with `Content-Range`, and `Accept-Ranges` on the full response so Chromium
 * knows it may ask.
 */
export function registerBackgroundProtocol(): void {
  protocol.handle(BACKGROUND_SCHEME, async (request) => {
    const filePath = getBackgroundVideoPath();
    if (!filePath) return new Response(null, { status: 404 });
    try {
      const { size } = await fs.promises.stat(filePath);
      const contentType =
        MIME_BY_EXTENSION[path.extname(filePath).slice(1).toLowerCase()] ?? 'application/octet-stream';
      const range = parseRange(request.headers.get('range'), size);

      if (range === 'unsatisfiable') {
        return new Response(null, { status: 416, headers: { 'content-range': `bytes */${size}` } });
      }

      if (!range) {
        return new Response(fileStream(filePath), {
          status: 200,
          headers: {
            'content-type': contentType,
            'content-length': String(size),
            'accept-ranges': 'bytes',
          },
        });
      }

      const { start, end } = range;
      return new Response(fileStream(filePath, start, end), {
        status: 206,
        headers: {
          'content-type': contentType,
          'content-length': String(end - start + 1),
          'content-range': `bytes ${start}-${end}/${size}`,
          'accept-ranges': 'bytes',
        },
      });
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
