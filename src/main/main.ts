import { app, BrowserWindow } from 'electron';
import path from 'path';
import { registerIpcHandlers } from './ipcHandlers';
import { createTray, setTrayMainWindow } from './tray';
import { setMainWindowForAuth } from './services/awsAuthService';
import { startScheduler } from './services/refreshScheduler';
import { getSettings } from './services/settingsService';
import { initAutoUpdater } from './services/autoUpdater';

let mainWindow: BrowserWindow | null = null;
let tray: ReturnType<typeof createTray> | null = null;
let isQuitting = false;

const isDev = process.env.NODE_ENV === 'development' || !app.isPackaged;

// Single instance: second launch focuses the existing window (or restores from tray)
const gotTheLock = app.requestSingleInstanceLock();
if (!gotTheLock) {
  app.quit();
} else {
  app.on('second-instance', () => {
    const win = mainWindow;
    if (win && !win.isDestroyed()) {
      if (win.isMinimized()) win.restore();
      win.show();
      win.focus();
    }
  });

  function createWindow(): BrowserWindow {
    const isWin = process.platform === 'win32';
    const win = new BrowserWindow({
      width: 1200,
      height: 800,
      minWidth: 800,
      minHeight: 600,
      frame: !isWin,
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
      win.webContents.openDevTools();
    } else {
      win.loadFile(path.join(__dirname, '../renderer/index.html'));
    }

    win.once('ready-to-show', () => win.show());
    return win;
  }

  app.whenReady().then(() => {
    const settings = getSettings();
    try {
      app.setLoginItemSettings({ openAtLogin: settings.launchAtStartup });
    } catch {
      // ignore on unsupported platforms
    }

    mainWindow = createWindow();
    setMainWindowForAuth(mainWindow);
    tray = createTray(mainWindow);
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
