import { BrowserWindow } from 'electron';

let rendererWindow: BrowserWindow | null = null;

export function setRendererWindow(win: BrowserWindow | null): void {
  rendererWindow = win;
}

export function sendToRenderer(channel: string, ...args: unknown[]): void {
  const w = rendererWindow;
  if (w && !w.isDestroyed()) {
    w.webContents.send(channel, ...args);
  }
}
