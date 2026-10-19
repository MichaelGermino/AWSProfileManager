import dotenv from 'dotenv';

// Load .env from project root (not committed; used for API keys etc.)
dotenv.config();

import { app, BrowserWindow, nativeImage, protocol } from 'electron';
import fs from 'fs';
import path from 'path';
import { registerIpcHandlers } from './ipcHandlers';
import { createTray, setTrayMainWindow } from './tray';
import { setMainWindowForAuth } from './services/awsAuthService';
import { setParentWindowForSso, setSsoProgressSink } from './services/identityCenterService';
import { applyEnterpriseTls } from './services/enterpriseTls';
import { setRendererWindow } from './services/ipcBridge';
import { startScheduler } from './services/refreshScheduler';
import { getAppDataPath } from './services/profileStorage';
import { getSettings } from './services/settingsService';
import { initAutoUpdater } from './services/autoUpdater';
import {
  getAppIconDataUrl,
  getDockIconPath,
  getSplashIconRelativePath,
  getWindowIconPath,
} from './services/appIcon';
import { syncShortcutIcons } from './services/shortcutIcon';
import { refreshTrayIcon } from './tray';
import {
  BACKGROUND_SCHEME_PRIVILEGES,
  registerBackgroundProtocol,
} from './services/backgroundMedia';

// Must run BEFORE app ready — privileges cannot be granted to a scheme once the protocol layer
// has been set up, and without `stream` the <video> element cannot range-request the file.
protocol.registerSchemesAsPrivileged([BACKGROUND_SCHEME_PRIVILEGES]);

let mainWindow: BrowserWindow | null = null;
let splashWindow: BrowserWindow | null = null;
let tray: ReturnType<typeof createTray> | null = null;
let isQuitting = false;

const isDev = process.env.NODE_ENV === 'development' || !app.isPackaged;
const SHORTCUT_SYNC_DELAY_MS = 5000;

function getSplashPath(): string {
  return path.join(__dirname, '../../resources/splash.html');
}

function setDockIcon(): void {
  const png = getDockIconPath();
  if (png) app.dock?.setIcon(nativeImage.createFromPath(png));
}

/**
 * Re-applies the selected app icon everywhere a running app can change it: every window's icon
 * (Windows title bar and taskbar button), the macOS Dock, the tray, the renderer's title bar, and
 * the Windows Start menu / desktop / pinned shortcuts.
 * Called after Settings → Appearance changes the icon or the tray options.
 */
export function applyAppIconChange(): void {
  syncShortcutIcons();
  const ico = getWindowIconPath();
  if (ico) {
    for (const w of BrowserWindow.getAllWindows()) {
      if (!w.isDestroyed()) w.setIcon(ico);
    }
  }
  if (process.platform === 'darwin') setDockIcon();
  refreshTrayIcon(true);
  if (mainWindow && !mainWindow.isDestroyed()) {
    mainWindow.webContents.send('app:iconChanged', getAppIconDataUrl());
  }
}

// Single instance: second launch focuses the existing window (or restores from tray)
const gotTheLock = app.requestSingleInstanceLock();
if (!gotTheLock) {
  app.quit();
} else {
  function closeSplashAndShowMain(): void {
    if (splashWindow && !splashWindow.isDestroyed()) {
      splashWindow.close();
      splashWindow = null;
    }
    const win = mainWindow;
    if (win && !win.isDestroyed()) {
      if (win.isMinimized()) win.restore();
      win.show();
      win.focus();
    }
  }

  app.on('second-instance', () => closeSplashAndShowMain());

  /**
   * Floor on how long the splash stays up, purely so a fast start doesn't flash it on and off.
   * The main window appears at max(time-to-ready, this) — so anything above the perception
   * threshold is dead time added to every launch. 5s here meant a 1s startup still took 5s.
   */
  const SPLASH_MIN_MS = 700;

  /**
   * Startup timings go to the console AND to `startup-log.txt` in app data.
   *
   * A packaged Windows app is a GUI-subsystem binary, so stdout is not attached to the terminal
   * that launched it — console.log alone is invisible in exactly the build whose timings matter
   * most. The file is rewritten each launch, so it stays a few lines.
   */
  const startupLines: string[] = [];
  function startupLog(message: string): void {
    const line = `[startup] ${message}`;
    startupLines.push(line);
    console.log(line);
  }
  function flushStartupLog(): void {
    try {
      const dir = getAppDataPath();
      if (!fs.existsSync(dir)) fs.mkdirSync(dir, { recursive: true });
      fs.writeFileSync(
        path.join(dir, 'startup-log.txt'),
        `${new Date().toISOString()}  packaged=${app.isPackaged}\n${startupLines.join('\n')}\n`,
        'utf-8'
      );
    } catch {
      // diagnostics only — never let this break startup
    }
  }

  function createSplash(onShown?: () => void): BrowserWindow | null {
    const splashPath = getSplashPath();
    if (!fs.existsSync(splashPath)) return null;
    const iconPath = getWindowIconPath();
    const splash = new BrowserWindow({
      width: 280,
      height: 280,
      frame: false,
      transparent: false,
      backgroundColor: '#1e1f22',
      show: false,
      resizable: false,
      ...(iconPath && { icon: iconPath }),
    });
    const tSplash = Date.now();
    splash.loadFile(splashPath, { query: { icon: getSplashIconRelativePath() } });
    splash.once('ready-to-show', () => {
      startupLog(`  splash visible +${Date.now() - tSplash}ms`);
      splash.show();
      onShown?.();
    });
    return splash;
  }

  function createWindow(onMainReady: () => void): BrowserWindow {
    const isWin = process.platform === 'win32';
    const isMac = process.platform === 'darwin';
    const iconPath = getWindowIconPath();
    const win = new BrowserWindow({
      width: 1200,
      height: 800,
      minWidth: 800,
      minHeight: 600,
      // Windows: frameless, the renderer draws its own title bar with min/max/close.
      // macOS: keep the native traffic lights but hide the title so the renderer's bar runs
      // edge to edge under them — same look as Windows, with the controls where Mac users
      // expect them. The renderer pads its bar on the left to clear the lights.
      frame: !isWin,
      ...(isMac && { titleBarStyle: 'hiddenInset' as const }),
      ...(iconPath && { icon: iconPath }),
      webPreferences: {
        preload: path.join(__dirname, '../preload/index.js'),
        nodeIntegration: false,
        contextIsolation: true,
      },
      show: false,
      backgroundColor: '#1e1f22',
    });

    if (isDev) {
      win.loadURL('http://localhost:5173');
      // On by default, as before. Set DEVTOOLS=0 to launch without it — useful when timing
      // startup, since devtools competes for CPU with Vite's on-demand module transforms.
      if (process.env.DEVTOOLS !== '0') win.webContents.openDevTools();
    } else {
      win.loadFile(path.join(__dirname, '../renderer/index.html'));
    }

    const t = Date.now();
    win.webContents.once('dom-ready', () => startupLog(`  dom-ready +${Date.now() - t}ms`));
    win.webContents.once('did-fail-load', (_e, code, desc) =>
      startupLog(`  did-fail-load +${Date.now() - t}ms (${code} ${desc})`)
    );

    /**
     * Treat the window as ready on whichever of these arrives first.
     *
     * `ready-to-show` is the conventional signal, but it fires on first *paint* — and a window
     * created with `show: false` can go unpainted indefinitely, in which case it never fires and
     * startup waits out the whole FALLBACK_SHOW_MS. `did-finish-load` means the page is loaded,
     * which is a sufficient basis to show it. The log names the winner so this stays diagnosable.
     */
    let signalled = false;
    const signalReady = (source: string) => {
      if (signalled) return;
      signalled = true;
      startupLog(`  ready via ${source} +${Date.now() - t}ms`);
      onMainReady();
    };
    win.once('ready-to-show', () => signalReady('ready-to-show'));
    win.webContents.once('did-finish-load', () => signalReady('did-finish-load'));

    return win;
  }

  app.whenReady().then(() => {
    // Startup timings. Cheap, and the only way to tell an actually-slow step from the splash's
    // artificial minimum — which is what "startup is slow" turned out to be the first time.
    const tReady = Date.now();
    const sinceLaunch = () => `${Date.now() - tReady}ms`;

    // Must run before any HTTPS call (auto-updater, scheduler, axios, AWS SDK).
    // Extends Node's TLS trust to include OS-provisioned CAs so the app works on
    // corporate machines behind TLS-inspecting proxies.
    const tTls = Date.now();
    applyEnterpriseTls();
    startupLog(`enterprise TLS ${Date.now() - tTls}ms`);

    // Serves the user's background video to the renderer; cheap, just installs a handler.
    registerBackgroundProtocol();

    const settings = getSettings();
    applyLaunchAtStartup(settings.launchAtStartup);

    // Dev builds on macOS show Electron's default dock icon, and a packaged build's bundle icon is
    // always Classic; either way a chosen design has to be set at runtime. A packaged build on
    // Classic needs nothing (the call would be harmless).
    if (process.platform === 'darwin' && (isDev || (settings.appIcon ?? 'classic') !== 'classic')) {
      setDockIcon();
    }

    let mainReady = false;
    let splashMinElapsed = false;
    const tryShowMain = () => {
      if (!mainReady || !splashMinElapsed || !mainWindow) return;
      clearTimeout(fallbackTimer);
      startupLog(`window shown at ${sinceLaunch()}`);
      flushStartupLog();
      closeSplashAndShowMain();
    };

    // If main window never becomes ready (e.g. slow/hung load on some machines), show it anyway after 15s
    const FALLBACK_SHOW_MS = 15_000;
    const fallbackTimer = setTimeout(() => {
      mainReady = true;
      splashMinElapsed = true;
      tryShowMain();
    }, FALLBACK_SHOW_MS);

    splashWindow = createSplash(() => {
      setTimeout(() => {
        splashMinElapsed = true;
        tryShowMain();
      }, SPLASH_MIN_MS);
    });
    if (!splashWindow) {
      splashMinElapsed = true;
    }
    mainWindow = createWindow(() => {
      mainReady = true;
      startupLog(`renderer ready at ${sinceLaunch()}`);
      clearTimeout(fallbackTimer);
      tryShowMain();
    });
    setRendererWindow(mainWindow);
    setMainWindowForAuth(mainWindow);
    // The SSO sign-in window parents to the main window, and progress goes back to the renderer.
    setParentWindowForSso(mainWindow);
    setSsoProgressSink((progress) => {
      if (mainWindow && !mainWindow.isDestroyed()) {
        mainWindow.webContents.send('sso:loginProgress', progress);
      }
    });
    tray = createTray(mainWindow, closeSplashAndShowMain);
    registerIpcHandlers(mainWindow);
    initAutoUpdater(getMainWindow);
    startScheduler();
    // An update reinstalls and resets the Start menu / desktop shortcuts to the .exe icon. Deferred
    // so the shortcut I/O never competes with the first paint.
    setTimeout(syncShortcutIcons, SHORTCUT_SYNC_DELAY_MS);

    if (settings.startMinimizedToTray) {
      mainWindow?.hide();
    }

    mainWindow?.on('close', (event) => {
      if (!isQuitting) {
        event.preventDefault();
        mainWindow?.hide();
      }
    });

    mainWindow?.on('closed', () => {
      mainWindow = null;
      setRendererWindow(null);
      setTrayMainWindow(null);
    });
  });

  app.on('window-all-closed', () => {
    if (process.platform !== 'darwin') {
      app.quit();
    }
  });

  // macOS: clicking the Dock icon while the window is hidden (close-to-tray, or Cmd+H) brings it
  // back. Without this the Dock click does nothing and the only way back in is the tray menu.
  app.on('activate', () => {
    closeSplashAndShowMain();
  });

  app.on('before-quit', () => {
    isQuitting = true;
    tray?.destroy();
  });
}

declare global {
  namespace NodeJS {
    interface Global {
      mainWindow: BrowserWindow | null;
    }
  }
}

export function getMainWindow(): BrowserWindow | null {
  return mainWindow;
}

/**
 * Register (or unregister) the app as a login item — only for a PACKAGED build. In development
 * the "app" is node_modules' bare Electron binary, and registering that adds a login item named
 * "Electron" that opens an empty window at every sign-in (on macOS it also lands in
 * System Settings → Login Items with no obvious owner).
 */
export function applyLaunchAtStartup(enabled: boolean): void {
  if (!app.isPackaged) return;
  try {
    // Only touch the registration when it would change. On macOS 13+ this goes through
    // SMAppService, and unregistering an app that was never registered (or registering one that
    // is not in /Applications) logs "Unable to set login item: Operation not permitted" on every
    // launch and every settings save — noise that looks like a real failure.
    if (app.getLoginItemSettings().openAtLogin === enabled) return;
    app.setLoginItemSettings({ openAtLogin: enabled });
  } catch {
    // ignore on unsupported platforms
  }
}
