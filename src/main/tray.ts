import { Tray, Menu, nativeImage, app } from 'electron';
import path from 'path';
import { getProfiles } from './services/profileStorage';
import { getSettings } from './services/settingsService';
import { profileMenuLabel } from './services/dashboardService';
import { refreshProfile, refreshAllProfiles } from './services/awsAuthService';
import { setRefreshPaused } from './services/refreshScheduler';
import { openConsoleForProfile } from './services/consoleSignIn';
import {
  showTrayHudBusy,
  showTrayHudError,
  hideTrayHud,
  destroyTrayHud,
  prewarmTrayHud,
} from './services/trayHud';

let tray: Tray | null = null;
let mainWindowRef: Electron.BrowserWindow | null = null;
let onOpenFromTray: (() => void) | null = null;

/**
 * Console launches currently in flight, by profile id.
 *
 * Opening the console usually has to refresh credentials first, which takes seconds. The menu
 * dismisses on click, so without this a user who reopens it sees no sign anything is happening and
 * clicks again — which federates twice and opens two console windows.
 */
const consoleLaunchesInFlight = new Set<string>();

/**
 * Launch the console for a profile with progress feedback pinned to the tray icon.
 *
 * Errors go to the HUD rather than a native Notification: the window is usually hidden when the
 * tray is in use, and Windows Focus Assist can suppress a toast outright, which turned a real
 * failure into a click that appeared to do nothing.
 */
async function openConsoleFromTray(profileId: string, label: string): Promise<void> {
  if (consoleLaunchesInFlight.has(profileId)) return;
  consoleLaunchesInFlight.add(profileId);
  updateTrayMenu();
  showTrayHudBusy(tray, 'Opening console…', label);

  try {
    const result = await openConsoleForProfile(profileId);
    if (result.success) hideTrayHud();
    else showTrayHudError(tray, 'Could not open console', result.error);
  } catch (err) {
    showTrayHudError(tray, 'Could not open console', err instanceof Error ? err.message : String(err));
  } finally {
    consoleLaunchesInFlight.delete(profileId);
    updateTrayMenu();
  }
}

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
   * preference, so this is just a call. Progress and failures are reported by the tray HUD — see
   * openConsoleFromTray.
   */
  const consoleSubmenu: Electron.MenuItemConstructorOptions[] =
    profiles.length > 0
      ? profiles.map((p) => {
          const label = profileMenuLabel(p, displayNames);
          const inFlight = consoleLaunchesInFlight.has(p.id);
          return {
            label: inFlight ? `${label}  (opening…)` : label,
            enabled: !inFlight,
            click: () => {
              void openConsoleFromTray(p.id, label);
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
  prewarmTrayHud();
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
  destroyTrayHud();
  if (tray) {
    tray.destroy();
    tray = null;
  }
  mainWindowRef = null;
}
