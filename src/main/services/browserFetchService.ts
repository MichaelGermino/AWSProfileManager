/**
 * Fetch HTML using a hidden BrowserWindow so the request uses Chromium's
 * network stack (system certs, proxy, same as the app). Use for AWS CLI docs
 * when renderer fetch fails due to CORS or context.
 */

import { BrowserWindow } from 'electron';

const TIMEOUT_MS = 20000;

export function fetchHtmlWithBrowser(url: string): Promise<string> {
  return new Promise((resolve, reject) => {
    const win = new BrowserWindow({
      show: false,
      webPreferences: {
        nodeIntegration: false,
        contextIsolation: true,
      },
    });

    let settled = false;
    function finish(err: Error | null, html?: string) {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      if (!win.isDestroyed()) win.destroy();
      if (err) reject(err);
      else if (html != null) resolve(html);
    }

    const timer = setTimeout(() => {
      finish(new Error(`Timeout loading ${url}`));
    }, TIMEOUT_MS);

    win.webContents.on('did-fail-load', (_e, errorCode, errorDescription, validatedUrl) => {
      if (validatedUrl === url) {
        finish(new Error(`Load failed: ${errorDescription} (${errorCode})`));
      }
    });

    win.loadURL(url, { userAgent: 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36' }).catch((err) => {
      finish(err);
    });

    win.webContents.once('did-finish-load', () => {
      if (settled) return;
      win.webContents
        .executeJavaScript('document.documentElement.outerHTML')
        .then((html: string) => finish(null, html))
        .catch((err: Error) => finish(err));
    });
  });
}
