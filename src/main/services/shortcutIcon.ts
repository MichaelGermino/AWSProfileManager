import fs from 'fs';
import path from 'path';
import { app, shell } from 'electron';
import { getAppDataPath } from './appPaths';
import { getSettings } from './settingsService';
import { getDesignIcoPath, resolveAppIconKey } from './appIcon';

/**
 * Points the app's Windows shortcuts (Start menu, desktop, pinned taskbar) at the selected icon.
 *
 * Windows search and the Start menu draw a shortcut's IconLocation, which the installer sets to the
 * .exe, so without this they stay Classic whatever the user picks. Two details matter:
 *
 * - The icon cannot point into app.asar (the shell cannot read it), so the design's .ico is copied
 *   to app data under a per-design file name. A new path also makes the shell's icon cache miss,
 *   where rewriting one fixed file would keep showing the old image.
 * - Every update reinstalls, and the installer rewrites the shortcuts back to the .exe icon. So
 *   this runs on every packaged launch, not only when the setting changes, and writes a shortcut
 *   only when its icon is actually wrong.
 *
 * Only shortcuts whose target is this .exe are touched. Classic points them back at the .exe.
 */
const SHORTCUT_ICONS_DIR = 'shortcut-icons';

function shortcutDirs(): string[] {
  const appData = app.getPath('appData');
  return [
    path.join(appData, 'Microsoft', 'Windows', 'Start Menu', 'Programs'),
    // Per-machine installs put the Start menu shortcut here; writing it can fail without admin.
    path.join(process.env.ProgramData ?? 'C:\\ProgramData', 'Microsoft', 'Windows', 'Start Menu', 'Programs'),
    app.getPath('desktop'),
    path.join(appData, 'Microsoft', 'Internet Explorer', 'Quick Launch', 'User Pinned', 'TaskBar'),
  ];
}

const samePath = (a: string, b: string) => path.resolve(a).toLowerCase() === path.resolve(b).toLowerCase();

/** Copies the design's .ico out of the asar, rewriting it only when the release shipped a different file. */
function materializeIcon(key: string, source: string): string {
  const dir = path.join(getAppDataPath(), SHORTCUT_ICONS_DIR);
  const dest = path.join(dir, `${key}.ico`);
  const data = fs.readFileSync(source);
  let current: Buffer | null = null;
  try {
    current = fs.readFileSync(dest);
  } catch {
    /* not copied yet */
  }
  if (!current || !current.equals(data)) {
    fs.mkdirSync(dir, { recursive: true });
    fs.writeFileSync(dest, data);
  }
  return dest;
}

/** Updates every .lnk in `dirs` that targets `exePath`. Returns how many were rewritten. */
export function syncShortcutIconsIn(dirs: string[], exePath: string, key: string): number {
  const ico = getDesignIcoPath(key);
  const icon = ico ? materializeIcon(key, ico) : exePath;
  let changed = 0;
  for (const dir of dirs) {
    let names: string[];
    try {
      names = fs.readdirSync(dir).filter((n) => n.toLowerCase().endsWith('.lnk'));
    } catch {
      continue;
    }
    for (const name of names) {
      const file = path.join(dir, name);
      try {
        const link = shell.readShortcutLink(file);
        if (!link.target || !samePath(link.target, exePath)) continue;
        // An empty icon means "the target's own icon", which is already right for Classic.
        const current = link.icon || exePath;
        if (samePath(current, icon) && (link.iconIndex ?? 0) === 0) continue;
        if (shell.writeShortcutLink(file, 'update', { target: link.target, icon, iconIndex: 0 })) changed++;
      } catch {
        // Not a readable shortcut, or not writable (a per-machine shortcut without admin).
      }
    }
  }
  return changed;
}

/** Packaged Windows builds only: a dev build's .exe is electron.exe, which no shortcut targets. */
export function syncShortcutIcons(): void {
  if (process.platform !== 'win32' || !app.isPackaged) return;
  try {
    const changed = syncShortcutIconsIn(shortcutDirs(), process.execPath, resolveAppIconKey(getSettings().appIcon));
    if (changed) console.log(`[appIcon] updated ${changed} shortcut icon(s)`);
  } catch (err) {
    console.warn('[appIcon] shortcut icon sync failed:', err instanceof Error ? err.message : err);
  }
}
