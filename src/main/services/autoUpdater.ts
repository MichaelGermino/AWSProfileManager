import { app, BrowserWindow } from 'electron';
import { autoUpdater } from 'electron-updater';
import { getSettings } from './settingsService';

const channel = 'update';

const GITHUB_OWNER = 'MichaelGermino';
const GITHUB_REPO = 'AWSProfileManager';

/** Read the "allow pre-release" toggle from settings and apply it to electron-updater
 *  before each check. Lets a developer flip it on in Settings → Debug → Developer options
 *  to test a pre-release build without restarting. */
function applyPrereleasePref(): void {
  try {
    autoUpdater.allowPrerelease = getSettings().allowPrerelease === true;
  } catch {
    // settings read failure is non-fatal; default behavior (no pre-releases) applies.
  }
}

export type UpdateStatus =
  | { type: 'available'; version: string }
  | { type: 'downloading'; percent: number }
  | { type: 'downloaded'; version: string }
  /** Install requested; the app is about to quit and the installer take over. */
  | { type: 'installing'; version?: string }
  | { type: 'error'; message: string }
  | { type: 'no-update' };

let lastUpdateStatus: UpdateStatus | null = null;

function send(win: BrowserWindow | null, payload: UpdateStatus): void {
  lastUpdateStatus = payload;
  if (win && !win.isDestroyed()) {
    win.webContents.send(channel, payload);
  }
}

export function getLastUpdateStatus(): UpdateStatus | null {
  return lastUpdateStatus;
}

export function initAutoUpdater(getMainWindow: () => BrowserWindow | null): void {
  if (!app.isPackaged) return;

  // Explicit feed URL so the updater always knows where to check (GitHub Releases)
  autoUpdater.setFeedURL({
    provider: 'github',
    owner: GITHUB_OWNER,
    repo: GITHUB_REPO,
  });

  autoUpdater.autoDownload = true;
  autoUpdater.autoInstallOnAppQuit = true;

  autoUpdater.on('update-available', (info) => {
    send(getMainWindow(), { type: 'available', version: info.version });
  });

  autoUpdater.on('download-progress', (progress) => {
    send(getMainWindow(), { type: 'downloading', percent: progress.percent });
  });

  autoUpdater.on('update-downloaded', (info) => {
    send(getMainWindow(), { type: 'downloaded', version: info.version });
  });

  autoUpdater.on('update-not-available', () => {
    send(getMainWindow(), { type: 'no-update' });
  });

  autoUpdater.on('error', (err) => {
    send(getMainWindow(), { type: 'error', message: err.message });
  });

  const doCheck = () => {
    applyPrereleasePref();
    autoUpdater.checkForUpdates().catch((err) => {
      send(getMainWindow(), { type: 'error', message: err?.message ?? 'Update check failed' });
    });
  };

  // Deferred rather than immediate: the first check does DNS + a TLS handshake to GitHub, and
  // behind a corporate proxy that can be slow. Nothing waits on the result, so keep it off the
  // startup path entirely instead of racing it with window creation.
  const INITIAL_CHECK_DELAY_MS = 5_000;
  setTimeout(doCheck, INITIAL_CHECK_DELAY_MS);

  const ONE_HOUR_MS = 60 * 60 * 1000;
  setInterval(doCheck, ONE_HOUR_MS);
}

/** Call from renderer to manually trigger an update check. Resolves with result for UI (no-update, available, or error). */
export function checkForUpdatesNow(getMainWindow: () => BrowserWindow | null): Promise<UpdateStatus> {
  return new Promise((resolve) => {
    const win = getMainWindow();
    const once = (payload: UpdateStatus) => {
      if (win && !win.isDestroyed()) win.webContents.send(channel, payload);
      resolve(payload);
    };
    const onAvailable = (info: { version: string }) => {
      once({ type: 'available', version: info.version });
      cleanup();
    };
    const onNotAvailable = () => {
      once({ type: 'no-update' });
      cleanup();
    };
    const onError = (err: Error) => {
      once({ type: 'error', message: err?.message ?? 'Update check failed' });
      cleanup();
    };
    const cleanup = () => {
      autoUpdater.off('update-available', onAvailable);
      autoUpdater.off('update-not-available', onNotAvailable);
      autoUpdater.off('error', onError);
    };
    autoUpdater.once('update-available', onAvailable);
    autoUpdater.once('update-not-available', onNotAvailable);
    autoUpdater.once('error', onError);
    applyPrereleasePref();
    autoUpdater.checkForUpdates().catch((err) => {
      onError(err);
    });
  });
}

/**
 * How long the renderer gets to paint the "Installing update" overlay before the quit starts.
 * Quitting tears down the tray, the terminals' PTYs and every window, and while that runs the
 * window cannot change — without a visible state first, it just looks frozen, and users clicked
 * the button again and again.
 */
const PAINT_BEFORE_QUIT_MS = 300;
/**
 * quitAndInstall() returns without quitting when the installer cannot be started (a missing or
 * removed download, for one). If the app is still running after this long, it never will be.
 */
const INSTALL_START_TIMEOUT_MS = 15_000;

let installRequested = false;

export function installUpdateAndRestart(getMainWindow: () => BrowserWindow | null): void {
  // Repeat clicks while the quit is under way must not start a second install.
  if (installRequested) return;
  installRequested = true;
  const version = lastUpdateStatus?.type === 'downloaded' ? lastUpdateStatus.version : undefined;
  send(getMainWindow(), { type: 'installing', version });

  let quitStarted = false;
  app.once('before-quit', () => {
    quitStarted = true;
  });

  setTimeout(() => {
    // electron-updater reports a failed start through its 'error' event, which the listener in
    // initAutoUpdater already forwards to the renderer; just allow another attempt.
    const onError = () => {
      clearTimeout(fallback);
      installRequested = false;
    };
    const fallback = setTimeout(() => {
      autoUpdater.off('error', onError);
      if (quitStarted) return;
      installRequested = false;
      send(getMainWindow(), {
        type: 'error',
        message: 'The update could not be installed. Check for updates to download it again.',
      });
    }, INSTALL_START_TIMEOUT_MS);
    autoUpdater.once('error', onError);
    autoUpdater.quitAndInstall(false, true);
  }, PAINT_BEFORE_QUIT_MS);
}
