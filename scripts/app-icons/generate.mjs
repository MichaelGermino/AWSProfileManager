/**
 * Renders every design in designs.browser.js to the PNG/ICO files the app loads at runtime.
 *
 *   npm run icons
 *
 * Runs under Electron rather than Node so Chromium can rasterize the SVGs (filters, masks and
 * gradients included) with no extra dependency. The output is committed: packaging never runs
 * this, so a build machine needs nothing new.
 *
 * Per design, in resources/app-icons/<key>/:
 *   icon.ico          16-256 px; the heavier "small" master below 40 px
 *   mac.png           512 px on the macOS squircle grid (Dock; 256 pt @2x is the largest it draws)
 *   ui.png            128 px, for the renderer (title bar, picker) and the splash
 *   tray-<n>.png      color tray icon at 16/18/32/36 px (18/36 = macOS menu bar @1x/@2x)
 *   tray-mono-<n>.png white glyph on transparent, recolored at runtime for light taskbars and
 *                     turned into a template image on macOS
 * plus resources/app-icons/manifest.json listing key, name and style in display order.
 */
import { app, BrowserWindow } from 'electron';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const here = path.dirname(fileURLToPath(import.meta.url));
const outDir = path.resolve(here, '../../resources/app-icons');
const ICO_SIZES = [16, 20, 24, 32, 40, 48, 64, 256];
const TRAY_SIZES = [16, 18, 32, 36];

/** PNG-compressed ICO (supported since Vista). Width/height byte 0 means 256. */
function buildIco(frames) {
  const header = Buffer.alloc(6 + 16 * frames.length);
  header.writeUInt16LE(0, 0);
  header.writeUInt16LE(1, 2);
  header.writeUInt16LE(frames.length, 4);
  let offset = header.length;
  frames.forEach(({ size, png }, i) => {
    const e = 6 + i * 16;
    header.writeUInt8(size >= 256 ? 0 : size, e);
    header.writeUInt8(size >= 256 ? 0 : size, e + 1);
    header.writeUInt8(0, e + 2);
    header.writeUInt8(0, e + 3);
    header.writeUInt16LE(1, e + 4);
    header.writeUInt16LE(32, e + 6);
    header.writeUInt32LE(png.length, e + 8);
    header.writeUInt32LE(offset, e + 12);
    offset += png.length;
  });
  return Buffer.concat([header, ...frames.map((f) => f.png)]);
}

const RASTERIZE = `
globalThis.rasterize = async (svg, size) => {
  const img = new Image();
  img.src = 'data:image/svg+xml;charset=utf-8,' + encodeURIComponent(svg);
  await img.decode();
  const c = document.createElement('canvas');
  c.width = c.height = size;
  c.getContext('2d').drawImage(img, 0, 0, size, size);
  return c.toDataURL('image/png');
};`;

app.whenReady().then(async () => {
  const win = new BrowserWindow({ show: false, webPreferences: { backgroundThrottling: false } });
  await win.loadURL('data:text/html,<!doctype html><meta charset="utf-8">');
  await win.webContents.executeJavaScript(fs.readFileSync(path.join(here, 'designs.browser.js'), 'utf-8') + RASTERIZE + ';0');
  const designs = await win.webContents.executeJavaScript('APP_ICON_DESIGNS');

  const render = async (key, variant, size, color) => {
    const args = [key, variant, size, color ?? null].map((a) => JSON.stringify(a)).join(', ');
    const dataUrl = await win.webContents.executeJavaScript(`rasterize(renderAppIconSvg(${args}), ${size})`);
    return Buffer.from(dataUrl.split(',')[1], 'base64');
  };

  fs.mkdirSync(outDir, { recursive: true });
  for (const d of designs) {
    const dir = path.join(outDir, d.key);
    fs.mkdirSync(dir, { recursive: true });
    const frames = [];
    for (const size of ICO_SIZES) frames.push({ size, png: await render(d.key, size <= 32 ? 'small' : 'win', size) });
    fs.writeFileSync(path.join(dir, 'icon.ico'), buildIco(frames));
    fs.writeFileSync(path.join(dir, 'mac.png'), await render(d.key, 'mac', 512));
    fs.writeFileSync(path.join(dir, 'ui.png'), await render(d.key, 'win', 128));
    for (const size of TRAY_SIZES) {
      fs.writeFileSync(path.join(dir, `tray-${size}.png`), await render(d.key, 'small', size));
      fs.writeFileSync(path.join(dir, `tray-mono-${size}.png`), await render(d.key, 'mono', size, '#fff'));
    }
    console.log(`  ${d.key}`);
  }
  fs.writeFileSync(path.join(outDir, 'manifest.json'), JSON.stringify(designs, null, 2) + '\n');
  console.log(`Wrote ${designs.length} designs to ${outDir}`);
  app.quit();
}).catch((err) => {
  console.error(err);
  app.exit(1);
});
