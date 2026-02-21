import { app, BrowserWindow } from 'electron';
import { autoUpdater } from 'electron-updater';

const channel = 'update';

export type UpdateStatus =
  | { type: 'available'; version: string }
  | { type: 'downloading'; percent: number }
  | { type: 'downloaded'; version: string }
  | { type: 'error'; message: string };

function send(win: BrowserWindow | null, payload: UpdateStatus): void {
  if (win && !win.isDestroyed()) {
    win.webContents.send(channel, payload);
  }
}

export function initAutoUpdater(getMainWindow: () => BrowserWindow | null): void {
  if (!app.isPackaged) return;

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

  autoUpdater.on('error', (err) => {
    send(getMainWindow(), { type: 'error', message: err.message });
  });

  autoUpdater.checkForUpdates().catch(() => {
    // Ignore: no update or network error; UI shows nothing
  });
}

export function installUpdateAndRestart(): void {
  autoUpdater.quitAndInstall(false, true);
}
