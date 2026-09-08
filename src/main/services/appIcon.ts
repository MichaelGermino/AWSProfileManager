import fs from 'fs';
import path from 'path';

/**
 * Icon for secondary windows. Windows uses it for the title bar and taskbar entry; without it
 * Electron falls back to its own default icon, which makes an app window look like it isn't ours.
 *
 * Path is relative to the compiled location (dist/main/services), and resolves to the repo/asar
 * root `resources/` because electron-builder packages `resources/**` alongside `dist/**`.
 */
export function getWindowIconPath(): string | undefined {
  if (process.platform !== 'win32') return undefined;
  const icoPath = path.join(__dirname, '../../../resources/icon.ico');
  return fs.existsSync(icoPath) ? icoPath : undefined;
}
