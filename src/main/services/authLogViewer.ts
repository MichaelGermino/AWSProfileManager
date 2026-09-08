import path from 'path';
import { pathToFileURL } from 'url';
import { BrowserWindow } from 'electron';
import { getWindowIconPath } from './appIcon';

let logViewerWindow: BrowserWindow | null = null;

const AUTH_AUDIT_UPDATED = 'logs:authAuditUpdated';

/** Call after auth audit entries change so an open viewer can refresh. */
export function notifyAuthAuditLogChanged(): void {
  if (!logViewerWindow || logViewerWindow.isDestroyed()) return;
  logViewerWindow.webContents.send(AUTH_AUDIT_UPDATED);
}

export function openAuthLogViewerWindow(parent: BrowserWindow | null): void {
  const htmlPath = path.join(__dirname, '../../../resources/auth-log-viewer.html');
  const preloadPath = path.join(__dirname, '../../preload/logViewer.js');
  const fileUrl = pathToFileURL(htmlPath).href;

  if (logViewerWindow && !logViewerWindow.isDestroyed()) {
    logViewerWindow.loadURL(fileUrl);
    logViewerWindow.focus();
    return;
  }

  const iconPath = getWindowIconPath();
  logViewerWindow = new BrowserWindow({
    width: 880,
    height: 640,
    parent: parent ?? undefined,
    title: 'Auth audit log',
    ...(iconPath && { icon: iconPath }),
    webPreferences: {
      preload: preloadPath,
      nodeIntegration: false,
      contextIsolation: true,
    },
  });
  logViewerWindow.loadURL(fileUrl);
  logViewerWindow.on('closed', () => {
    logViewerWindow = null;
  });
}
