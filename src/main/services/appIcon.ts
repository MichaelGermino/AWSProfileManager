import fs from 'fs';
import path from 'path';
import { nativeImage, nativeTheme, type NativeImage } from 'electron';
import { CLASSIC_APP_ICON, type AppIconOption, type TrayIconStatus } from '../../shared/types';
import { getSettings } from './settingsService';

/**
 * The selectable app icon (Settings → Appearance → App icon).
 *
 * "Classic" is the original resources/icon.* set and stays the default, so an update never changes
 * an existing user's icon. Every other design is a folder of pre-rendered files under
 * resources/app-icons/<key>/, produced by `npm run icons` from scripts/app-icons/designs.browser.js.
 *
 * Paths are relative to the compiled location (dist/main/services) and resolve to the repo/asar
 * root `resources/`, because electron-builder packages `resources/**` alongside `dist/**`.
 *
 * What a runtime switch can reach: window and taskbar-button icons, the macOS Dock while running,
 * and the tray. The installer, the .exe, Start menu / pinned shortcuts and the Finder icon are
 * fixed at build time and keep Classic.
 */
const RESOURCES = path.join(__dirname, '../../../resources');
const DESIGNS_DIR = path.join(RESOURCES, 'app-icons');

type Design = { key: string; name: string; style: string };
let manifest: Design[] | null = null;

function getDesigns(): Design[] {
  if (manifest) return manifest;
  try {
    manifest = JSON.parse(fs.readFileSync(path.join(DESIGNS_DIR, 'manifest.json'), 'utf-8')) as Design[];
  } catch {
    manifest = [];
  }
  return manifest;
}

/** An unknown key (a design removed in a later release, a hand-edited settings.json) falls back to Classic. */
export function resolveAppIconKey(key: string | undefined): string {
  return key && getDesigns().some((d) => d.key === key) ? key : CLASSIC_APP_ICON;
}

function currentKey(): string {
  return resolveAppIconKey(getSettings().appIcon);
}

function existing(p: string): string | undefined {
  return fs.existsSync(p) ? p : undefined;
}

function pngPathFor(key: string): string {
  return key === CLASSIC_APP_ICON ? path.join(RESOURCES, 'icon.png') : path.join(DESIGNS_DIR, key, 'ui.png');
}

/**
 * Icon for BrowserWindows. Windows uses it for the title bar and taskbar button; without it
 * Electron falls back to its own default icon, which makes an app window look like it isn't ours.
 * Other platforms take the window icon from the app bundle, so this is Windows-only.
 */
export function getWindowIconPath(): string | undefined {
  if (process.platform !== 'win32') return undefined;
  const key = currentKey();
  return key === CLASSIC_APP_ICON
    ? existing(path.join(RESOURCES, 'icon.ico'))
    : existing(path.join(DESIGNS_DIR, key, 'icon.ico'));
}

/** Splash image, relative to resources/ (the splash page is loaded from there). */
export function getSplashIconRelativePath(): string {
  const key = currentKey();
  return key === CLASSIC_APP_ICON ? 'icon.png' : `app-icons/${key}/ui.png`;
}

/** macOS Dock image for the selected design. Classic uses the padded square the bundle icon is built from. */
export function getDockIconPath(): string | undefined {
  const key = currentKey();
  return key === CLASSIC_APP_ICON
    ? existing(path.join(RESOURCES, 'icon-mac.png'))
    : existing(path.join(DESIGNS_DIR, key, 'mac.png'));
}

function dataUrlOf(file: string): string | null {
  if (!fs.existsSync(file)) return null;
  const img = nativeImage.createFromPath(file);
  return img.isEmpty() ? null : img.toDataURL();
}

/** The selected icon as a data URL, for the renderer's title bar. */
export function getAppIconDataUrl(): string | null {
  return dataUrlOf(pngPathFor(currentKey()));
}

/** Every choice, Classic first, with a preview image for the picker. */
export function listAppIcons(): AppIconOption[] {
  return [{ key: CLASSIC_APP_ICON, name: 'Classic', style: 'Original' }, ...getDesigns()].map((d) => ({
    ...d,
    dataUrl: dataUrlOf(pngPathFor(d.key)),
  }));
}

// ---------------------------------------------------------------------------------------------
// Tray
// ---------------------------------------------------------------------------------------------

/** Badge colors, as [r, g, b]. 'ok' never draws a badge. */
const BADGE_RGB: Record<Exclude<TrayIconStatus, 'ok'>, [number, number, number]> = {
  warn: [0xff, 0xb0, 0x20],
  err: [0xff, 0x4d, 0x4f],
  paused: [0x8a, 0x8f, 0xa3],
};

/**
 * Edits a PNG's pixels in place. NativeImage bitmaps are BGRA with premultiplied alpha on both
 * Windows and macOS, which is what the blending below assumes.
 */
function editPixels(png: Buffer, edit: (px: Buffer, size: number) => void): Buffer {
  const img = nativeImage.createFromBuffer(png);
  const { width, height } = img.getSize();
  const px = Buffer.from(img.toBitmap());
  edit(px, width);
  return nativeImage.createFromBitmap(px, { width, height }).toPNG();
}

/** Monochrome sources are white; a light taskbar and a macOS template image need black. */
function toBlack(px: Buffer): void {
  for (let i = 0; i < px.length; i += 4) {
    px[i] = px[i + 1] = px[i + 2] = 0;
  }
}

/**
 * A status dot in the bottom-right corner, with a transparent ring knocked out around it so it
 * reads against any icon and any taskbar color. Antialiased by distance from the edge.
 */
function drawBadge(px: Buffer, size: number, rgb: [number, number, number]): void {
  const c = size * 0.78;
  const r = size * 0.22;
  const gap = Math.max(1, size * 0.07);
  for (let y = 0; y < size; y++) {
    for (let x = 0; x < size; x++) {
      const d = Math.hypot(x + 0.5 - c, y + 0.5 - c);
      const hole = Math.min(1, Math.max(0, r + gap + 0.5 - d));
      if (hole === 0) continue;
      const dot = Math.min(1, Math.max(0, r + 0.5 - d));
      const i = (y * size + x) * 4;
      const keep = 1 - hole;
      px[i] = Math.round(rgb[2] * dot + px[i] * keep * (1 - dot));
      px[i + 1] = Math.round(rgb[1] * dot + px[i + 1] * keep * (1 - dot));
      px[i + 2] = Math.round(rgb[0] * dot + px[i + 2] * keep * (1 - dot));
      px[i + 3] = Math.round(255 * dot + px[i + 3] * keep * (1 - dot));
    }
  }
}

/** Taskbar color decides the monochrome glyph's color on Windows. */
function windowsTaskbarIsDark(): boolean {
  return nativeTheme.shouldUseDarkColorsForSystemIntegratedUI ?? nativeTheme.shouldUseDarkColors;
}

/**
 * macOS menu-bar icons are 18pt; anything larger is scaled down blurrily, or on some versions
 * stretches the bar. Windows draws the tray at 16px per 100% scaling.
 */
const MAC_MENU_BAR_ICON_PT = 18;
const CLASSIC_WIN_TRAY_PX = 32;

/** Classic keeps exactly the tray image it always had (one resize of icon.png), plus the badge. */
function classicTrayImage(status: TrayIconStatus, badge: boolean): NativeImage {
  const icon = nativeImage.createFromPath(path.join(RESOURCES, 'icon.png'));
  if (icon.isEmpty()) return nativeImage.createEmpty();
  let resized: NativeImage;
  let scaleFactor = 1;
  if (process.platform === 'darwin') {
    const { width, height } = icon.getSize();
    const scale = (MAC_MENU_BAR_ICON_PT * 2) / Math.max(width, height);
    resized = icon.resize({ width: Math.round(width * scale), height: Math.round(height * scale) });
    scaleFactor = 2;
  } else {
    resized = icon.resize({ width: CLASSIC_WIN_TRAY_PX, height: CLASSIC_WIN_TRAY_PX });
  }
  let png = resized.toPNG();
  if (badge && status !== 'ok') {
    const { width, height } = resized.getSize();
    // The badge geometry assumes a square; the mac resize keeps icon.png's aspect ratio.
    if (width === height) png = editPixels(png, (px, size) => drawBadge(px, size, BADGE_RGB[status]));
  }
  return nativeImage.createFromBuffer(png, { scaleFactor });
}

/**
 * The tray image for the current settings and credential status.
 *
 * Built as a multi-resolution image (1x + 2x) so Windows picks the right one for the display's
 * scaling and macOS gets a crisp Retina menu-bar icon.
 */
export function buildTrayImage(status: TrayIconStatus): NativeImage {
  const settings = getSettings();
  const key = resolveAppIconKey(settings.appIcon);
  const badge = settings.trayStatusBadge !== false;
  if (key === CLASSIC_APP_ICON) return classicTrayImage(status, badge);

  const isMac = process.platform === 'darwin';
  const mono = settings.trayIconStyle === 'monochrome';
  const black = mono && (isMac || !windowsTaskbarIsDark());
  const [size1x, size2x] = isMac ? [18, 36] : [16, 32];

  const image = nativeImage.createEmpty();
  for (const [scaleFactor, size] of [[1, size1x], [2, size2x]] as const) {
    const file = path.join(DESIGNS_DIR, key, mono ? `tray-mono-${size}.png` : `tray-${size}.png`);
    if (!fs.existsSync(file)) continue;
    let png: Buffer = fs.readFileSync(file);
    if (black || (badge && status !== 'ok')) {
      png = editPixels(png, (px, n) => {
        if (black) toBlack(px);
        // A template image is alpha-only, so on macOS monochrome the dot is black like the glyph.
        if (badge && status !== 'ok') drawBadge(px, n, isMac && mono ? [0, 0, 0] : BADGE_RGB[status]);
      });
    }
    image.addRepresentation({ scaleFactor, buffer: png });
  }
  if (isMac && mono) image.setTemplateImage(true);
  return image;
}
