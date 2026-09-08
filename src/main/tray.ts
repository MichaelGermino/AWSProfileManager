import { Tray, Menu, nativeImage, app, Notification } from 'electron';
import path from 'path';
import { getProfiles } from './services/profileStorage';
import { getSettings } from './services/settingsService';
import { profileMenuLabel } from './services/dashboardService';
import { refreshProfile, refreshAllProfiles } from './services/awsAuthService';
import { setRefreshPaused } from './services/refreshScheduler';
import { openConsoleForProfile } from './services/consoleSignIn';

let tray: Tray | null = null;
let mainWindowRef: Electron.BrowserWindow | null = null;
let onOpenFromTray: (() => void) | null = null;

function buildContextMenu(): Menu {
  const profiles = getProfiles();
  // Read here rather than per item: one settings read per menu build, not per profile.
  const displayNames = getSettings().accountDisplayNames ?? {};
  const refreshSubmenu: Electron.MenuItemConstructorOptions[] = [
    {
      label: 'All',
      click: () => refreshAllProfiles(),
    },
    ...profiles.map((p) => ({
      label: profileMenuLabel(p, displayNames),
      click: () => {
        void refreshProfile(p.id);
      },
    })),
  ];

  /**
   * Opening the console refreshes stale credentials first and honors the user's browser
   * preference, so this is just a call. Failures need a native notification rather than the
   * renderer's in-app toast: the window is usually hidden when the tray is being used, so an
   * in-app message would go unseen and the click would look like it did nothing.
   */
  const notifyFailure = (profileName: string, error: string) => {
    if (!Notification.isSupported()) return;
    new Notification({ title: `Could not open AWS console for ${profileName}`, body: error }).show();
  };

  const consoleSubmenu: Electron.MenuItemConstructorOptions[] =
    profiles.length > 0
      ? profiles.map((p) => {
          const label = profileMenuLabel(p, displayNames);
          return {
            label,
            click: () => {
              void openConsoleForProfile(p.id).then((result) => {
                if (!result.success) notifyFailure(label, result.error);
              });
            },
          };
        })
      : [{ label: 'No profiles yet', enabled: false }];

  const openApp = () => {
    if (onOpenFromTray) onOpenFromTray();
    else {
      mainWindowRef?.show();
      mainWindowRef?.focus();
    }
  };

  const template: Electron.MenuItemConstructorOptions[] = [
    { label: 'Open App', click: openApp },
    { type: 'separator' },
    { label: 'Refresh profile', submenu: refreshSubmenu },
    { label: 'AWS Console', submenu: consoleSubmenu },
    {
      label: 'Pause Auto Refresh',
      click: () => {
        setRefreshPaused(true);
      },
    },
    {
      label: 'Resume Auto Refresh',
      click: () => {
        setRefreshPaused(false);
      },
    },
    { type: 'separator' },
    { label: 'Exit', click: () => app.quit() },
  ];

  return Menu.buildFromTemplate(template);
}

const TRAY_ICON_SIZE = 32;

export function createTray(mainWindow: Electron.BrowserWindow, onOpen?: () => void): Tray {
  mainWindowRef = mainWindow;
  onOpenFromTray = onOpen ?? null;
  const iconPath = path.join(__dirname, '../../resources/icon.png');
  let icon = nativeImage.createFromPath(iconPath);
  if (icon.isEmpty()) {
    icon = nativeImage.createEmpty();
  } else {
    icon = icon.resize({ width: TRAY_ICON_SIZE, height: TRAY_ICON_SIZE });
  }
  tray = new Tray(icon);
  tray.setToolTip('AWS Profile Manager');
  tray.setContextMenu(buildContextMenu());
  tray.on('double-click', () => (onOpenFromTray ?? (() => { mainWindow.show(); mainWindow.focus(); }))());
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
