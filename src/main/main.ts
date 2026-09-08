import dotenv from 'dotenv';

// Load .env from project root (not committed; used for API keys etc.)
dotenv.config();

import { app, BrowserWindow, nativeImage } from 'electron';
import fs from 'fs';
import path from 'path';
import { registerIpcHandlers } from './ipcHandlers';
import { createTray, setTrayMainWindow } from './tray';
import { setMainWindowForAuth } from './services/awsAuthService';
import { setParentWindowForSso, setSsoProgressSink } from './services/identityCenterService';
import { applyEnterpriseTls } from './services/enterpriseTls';
import { setRendererWindow } from './services/ipcBridge';
import { startScheduler } from './services/refreshScheduler';
import { getSettings } from './services/settingsService';
import { initAutoUpdater } from './services/autoUpdater';

let mainWindow: BrowserWindow | null = null;
let splashWindow: BrowserWindow | null = null;
let tray: ReturnType<typeof createTray> | null = null;
let isQuitting = false;

const isDev = process.env.NODE_ENV === 'development' || !app.isPackaged;

function getSplashPath(): string {
  return path.join(__dirname, '../../resources/splash.html');
}

function getAppIconPath(): string | undefined {
  if (process.platform !== 'win32') return undefined;
  const icoPath = path.join(__dirname, '../../resources/icon.ico');
  return fs.existsSync(icoPath) ? icoPath : undefined;
}

/** Returns the app icon PNG as a data URL for use in the renderer (e.g. sidebar). */
export function getAppIconDataUrl(): string | null {
  const pngPath = path.join(__dirname, '../../resources/icon.png');
  if (!fs.existsSync(pngPath)) return null;
  const img = nativeImage.createFromPath(pngPath);
  if (img.isEmpty()) return null;
  return img.toDataURL();
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

  function createSplash(onShown?: () => void): BrowserWindow | null {
    const splashPath = getSplashPath();
    if (!fs.existsSync(splashPath)) return null;
    const iconPath = getAppIconPath();
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
    splash.loadFile(splashPath);
    splash.once('ready-to-show', () => {
      splash.show();
      onShown?.();
    });
    return splash;
  }

  function createWindow(onMainReady: () => void): BrowserWindow {
    const isWin = process.platform === 'win32';
    const iconPath = getAppIconPath();
    const win = new BrowserWindow({
      width: 1200,
      height: 800,
      minWidth: 800,
      minHeight: 600,
      frame: !isWin,
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

    win.once('ready-to-show', onMainReady);
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
    console.log(`[startup] enterprise TLS ${Date.now() - tTls}ms`);

    const settings = getSettings();
    try {
      app.setLoginItemSettings({ openAtLogin: settings.launchAtStartup });
    } catch {
      // ignore on unsupported platforms
    }

    let mainReady = false;
    let splashMinElapsed = false;
    const tryShowMain = () => {
      if (!mainReady || !splashMinElapsed || !mainWindow) return;
      clearTimeout(fallbackTimer);
      console.log(`[startup] window shown at ${sinceLaunch()}`);
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
      console.log(`[startup] renderer ready at ${sinceLaunch()}`);
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
