import { Tray, Menu, nativeImage, app } from 'electron';
import path from 'path';
import { getProfiles } from './services/profileStorage';
import { refreshProfile } from './services/awsAuthService';
import { setRefreshPaused } from './services/refreshScheduler';

let tray: Tray | null = null;
let mainWindowRef: Electron.BrowserWindow | null = null;

function buildContextMenu(): Menu {
  const profiles = getProfiles();
  const refreshSubmenu: Electron.MenuItemConstructorOptions[] = [
    {
      label: 'All',
      click: async () => {
        for (const p of profiles) {
          try {
            await refreshProfile(p.id);
          } catch {
            // ignore per-profile errors
          }
        }
      },
    },
    ...profiles.map((p) => ({
      label: p.name,
      click: () => refreshProfile(p.id),
    })),
  ];

  const template: Electron.MenuItemConstructorOptions[] = [
    { label: 'Open App', click: () => { mainWindowRef?.show(); mainWindowRef?.focus(); } },
    { type: 'separator' },
    { label: 'Refresh profile', submenu: refreshSubmenu },
    {
      label: 'Pause Auto Refresh',
      click: () => {
        setRefreshPaused(true);
        mainWindowRef?.webContents?.send('scheduler:pausedChanged', true);
      },
    },
    {
      label: 'Resume Auto Refresh',
      click: () => {
        setRefreshPaused(false);
        mainWindowRef?.webContents?.send('scheduler:pausedChanged', false);
      },
    },
    { type: 'separator' },
    { label: 'Exit', click: () => app.quit() },
  ];

  return Menu.buildFromTemplate(template);
}

export function createTray(mainWindow: Electron.BrowserWindow): Tray {
  mainWindowRef = mainWindow;
  const iconPath = path.join(__dirname, '../../resources/tray-icon.png');
  let icon = nativeImage.createFromPath(iconPath);
  if (icon.isEmpty()) {
    icon = nativeImage.createEmpty();
  }
  tray = new Tray(icon);
  tray.setToolTip('AWS Profile Manager');
  tray.setContextMenu(buildContextMenu());
  tray.on('double-click', () => { mainWindow.show(); mainWindow.focus(); });
  return tray;
}

export function updateTrayMenu(): void {
  if (tray && !tray.isDestroyed()) {
    tray.setContextMenu(buildContextMenu());
  }
}

export function setTrayMainWindow(win: Electron.BrowserWindow | null): void {
  mainWindowRef = win;
}

export function getTray(): Tray | null {
  return tray;
}

export function destroyTray(): void {
  if (tray) {
    tray.destroy();
    tray = null;
  }
  mainWindowRef = null;
}
