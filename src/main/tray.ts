import { Tray, Menu, app, nativeTheme } from 'electron';
import { getProfiles } from './services/profileStorage';
import { getSettings } from './services/settingsService';
import { profileMenuLabel } from './services/dashboardService';
import { refreshProfile, refreshAllProfiles } from './services/awsAuthService';
import { getRefreshPauseState, setRefreshPaused } from './services/refreshScheduler';
import { buildTrayImage } from './services/appIcon';
import type { TrayIconStatus } from '../shared/types';
import { openConsoleForProfile } from './services/consoleSignIn';
import { isLocked } from './services/credentialStorage';
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
  // Belt and braces: the menu item is already disabled while locked, but a menu built before a
  // master-password reset would still be live.
  if (isLocked()) return;
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

  /**
   * The tray is reachable while the renderer is still on the unlock screen, so every action that
   * touches a profile has to be gated here too — otherwise the master password protects the window
   * and nothing else. Only "Open App" (which is how you reach the unlock prompt) and "Exit" stay
   * live. Items are shown-but-disabled rather than hidden, so the menu doesn't appear broken.
   */
  const locked = isLocked();

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
    ...(locked
      ? ([{ label: 'Locked — open the app to unlock', enabled: false }] as const)
      : []),
    { label: 'AWS Console', enabled: !locked, submenu: consoleSubmenu },
    {
      label: 'Pause Auto Refresh',
      enabled: !locked,
      click: () => {
        setRefreshPaused(true);
        refreshTrayIcon();
      },
    },
    {
      label: 'Resume Auto Refresh',
      enabled: !locked,
      click: () => {
        setRefreshPaused(false);
        refreshTrayIcon();
      },
    },
    { label: 'Refresh profile', enabled: !locked, submenu: refreshSubmenu },
    { type: 'separator' },
    { label: 'Exit', click: () => app.quit() },
  ];

  return Menu.buildFromTemplate(template);
}

/**
 * A profile counts as "expiring" for the badge only this close to expiry. The scheduler already
 * refreshes at 15 minutes out, so a wider window would flash the badge before every routine refresh.
 */
const EXPIRING_BADGE_MINUTES = 5;
/** Expiry moves with the clock rather than with any event, so the status is re-checked on a timer. */
const STATUS_CHECK_MS = 30_000;

let statusTimer: ReturnType<typeof setInterval> | null = null;
/** Status the current image shows; the image is only rebuilt when it changes (or on `force`). */
let shownStatus: TrayIconStatus | null = null;

function computeTrayStatus(): TrayIconStatus {
  const { paused, pausedDueToFailures } = getRefreshPauseState();
  if (pausedDueToFailures) return 'err';
  if (paused) return 'paused';
  const soon = Date.now() + EXPIRING_BADGE_MINUTES * 60_000;
  const expiring = getProfiles().some(
    (p) => p.autoRefresh && !!p.expiration && new Date(p.expiration).getTime() <= soon
  );
  return expiring ? 'warn' : 'ok';
}

const STATUS_TOOLTIP: Record<TrayIconStatus, string> = {
  ok: 'AWS Profile Manager',
  warn: 'AWS Profile Manager — credentials expiring',
  err: 'AWS Profile Manager — auto refresh stopped after failures',
  paused: 'AWS Profile Manager — auto refresh paused',
};

/**
 * Rebuilds the tray image when the credential status changes, or unconditionally after the icon
 * settings change (`force`). Cheap to call often: an unchanged status is a no-op.
 */
export function refreshTrayIcon(force = false): void {
  if (!tray || tray.isDestroyed()) return;
  const status = computeTrayStatus();
  if (!force && status === shownStatus) return;
  tray.setImage(buildTrayImage(status));
  tray.setToolTip(STATUS_TOOLTIP[status]);
  shownStatus = status;
}

export function createTray(mainWindow: Electron.BrowserWindow, onOpen?: () => void): Tray {
  mainWindowRef = mainWindow;
  onOpenFromTray = onOpen ?? null;
  const status = computeTrayStatus();
  tray = new Tray(buildTrayImage(status));
  shownStatus = status;
  statusTimer = setInterval(() => refreshTrayIcon(), STATUS_CHECK_MS);
  // A monochrome tray icon follows the Windows taskbar color, which can change while running.
  nativeTheme.on('updated', () => refreshTrayIcon(true));
  tray.setToolTip(STATUS_TOOLTIP[status]);
  tray.setContextMenu(buildContextMenu());
  tray.on('double-click', () => (onOpenFromTray ?? (() => { mainWindow.show(); mainWindow.focus(); }))());
  prewarmTrayHud();
  return tray;
}

export function updateTrayMenu(): void {
  if (tray && !tray.isDestroyed()) {
    tray.setContextMenu(buildContextMenu());
    refreshTrayIcon();
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
  if (statusTimer) {
    clearInterval(statusTimer);
    statusTimer = null;
  }
  if (tray) {
    tray.destroy();
    tray = null;
  }
  mainWindowRef = null;
}
