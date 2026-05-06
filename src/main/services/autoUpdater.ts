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

  doCheck();

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

export function installUpdateAndRestart(): void {
  autoUpdater.quitAndInstall(false, true);
}
