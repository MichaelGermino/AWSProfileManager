import fs from 'fs';
import path from 'path';
import { BrowserWindow, screen, Tray } from 'electron';

/**
 * A small progress card anchored to the tray icon.
 *
 * Why a window and not a spinner in the menu: a tray context menu is a native popup. Its `click`
 * handler does not fire until the menu has already dismissed, and Electron exposes no way to hold
 * it open or to mutate an item while it is showing — `tray.setContextMenu()` only affects the next
 * open. So the only place to put "working on it" feedback is a window of our own, positioned where
 * the menu just was.
 *
 * The window is transparent, frameless, non-focusable and click-through, so it reads as an overlay
 * rather than as an app window: it never steals focus from whatever the user is doing and never
 * swallows a click meant for the desktop beneath it.
 */

/** Size of the visible card. */
const CARD_W = 300;
const CARD_H = 64;
/**
 * Transparent margin around the card, so its CSS drop shadow has somewhere to fall. MUST match the
 * `body { padding }` in tray-hud.html — positioning is done on the card and converted to window
 * bounds through this, so a mismatch shows up as the card sitting the wrong distance from the
 * taskbar.
 */
const SHADOW_PAD = 10;
const WIDTH = CARD_W + SHADOW_PAD * 2;
const HEIGHT = CARD_H + SHADOW_PAD * 2;
/**
 * Visible gap between the card and the edge of the work area. Small on purpose: the tray menu has
 * already dismissed by the time the card appears, so there is nothing to clear — it should read as
 * sitting on the taskbar, like a system toast.
 */
const EDGE_MARGIN = 6;

/**
 * Wait this long before painting the busy card.
 *
 * A profile whose credentials are still valid skips the refresh entirely and opens the console in
 * a few hundred milliseconds. Showing a spinner for that is worse than showing nothing — it lands
 * as a flicker, or (before this delay existed) arrived after the browser was already up. If the
 * work finishes inside the delay the card is never created at all.
 */
const BUSY_SHOW_DELAY_MS = 350;
/**
 * Once painted, stay up at least this long. Without it, work that finishes just past the show
 * delay produces a one-frame blink.
 */
const MIN_VISIBLE_MS = 600;
const ERROR_DISMISS_MS = 6_000;
/**
 * Hard cap on the busy state. Opening a console can legitimately take a while — an Identity Center
 * profile with a lapsed session opens a sign-in window and waits on a human — but a spinner that
 * can outlive its promise would be a permanent artifact on screen if a caller ever forgot to
 * settle. This makes the failure mode "feedback disappears", not "feedback sticks forever".
 */
const MAX_BUSY_MS = 3 * 60_000;

let hud: BrowserWindow | null = null;
/** True once the page has loaded, so a paint can skip straight to showing. */
let ready = false;
let shownAt = 0;

/**
 * Bumped by every public call. Anything scheduled asynchronously captures the value it was created
 * with and no-ops once a newer call supersedes it.
 *
 * This is the fix for a stranded spinner: show and hide are both async (a pending page load, a
 * minimum-dwell timer), so without a generation check a hide that lands mid-show is simply undone
 * by the show finishing afterwards, and the card stays up forever with nothing left to dismiss it.
 */
let generation = 0;
let showTimer: NodeJS.Timeout | null = null;
let hideTimer: NodeJS.Timeout | null = null;
let dismissTimer: NodeJS.Timeout | null = null;

/** Resolves to the repo/asar root `resources/`, which electron-builder packages alongside `dist/**`. */
function getHudHtmlPath(): string | null {
  const htmlPath = path.join(__dirname, '../../../resources/tray-hud.html');
  return fs.existsSync(htmlPath) ? htmlPath : null;
}

function clamp(value: number, min: number, max: number): number {
  // max < min when the work area is narrower than the card; prefer the top-left corner then.
  return Math.max(min, Math.min(max, value));
}

/**
 * Place the card next to the tray icon, on the inward side of whichever edge the taskbar is docked
 * to.
 *
 * Everything here is computed on the visible card and converted to window bounds at the end.
 * Positioning the window directly double-counts SHADOW_PAD and leaves the card floating well clear
 * of the taskbar, which reads as misaligned next to a system toast.
 *
 * The tray sits *outside* the work area, so the clamp is what actually parks the card against the
 * taskbar edge; the initial offset only decides which side of the icon it tries first.
 */
function positionFor(tray: Tray): { x: number; y: number } {
  const bounds = tray.getBounds();
  const anchorX = bounds.x + bounds.width / 2;
  const anchorY = bounds.y + bounds.height / 2;
  const area = screen.getDisplayNearestPoint({
    x: Math.round(anchorX),
    y: Math.round(anchorY),
  }).workArea;

  const trayIsNearTop = anchorY < area.y + area.height / 2;
  const cardY = trayIsNearTop ? bounds.y + bounds.height : bounds.y - CARD_H;
  const cardX = Math.round(anchorX - CARD_W / 2);

  return {
    x: clamp(cardX, area.x + EDGE_MARGIN, area.x + area.width - CARD_W - EDGE_MARGIN) - SHADOW_PAD,
    y:
      clamp(Math.round(cardY), area.y + EDGE_MARGIN, area.y + area.height - CARD_H - EDGE_MARGIN) -
      SHADOW_PAD,
  };
}

/**
 * The page is loaded ONCE and thereafter updated in place. Reloading it per show meant every card
 * waited on a page load before it could be shown, which is what made the first one arrive late.
 */
function ensureWindow(): BrowserWindow | null {
  if (hud && !hud.isDestroyed()) return hud;
  const htmlPath = getHudHtmlPath();
  if (!htmlPath) return null;

  ready = false;
  hud = new BrowserWindow({
    width: WIDTH,
    height: HEIGHT,
    show: false,
    frame: false,
    transparent: true,
    // We draw the shadow in CSS; the native one would trace the transparent window, not the card.
    hasShadow: false,
    resizable: false,
    movable: false,
    minimizable: false,
    maximizable: false,
    fullscreenable: false,
    skipTaskbar: true,
    alwaysOnTop: true,
    focusable: false,
    webPreferences: {
      nodeIntegration: false,
      contextIsolation: true,
    },
  });
  // Purely informational, so clicks belong to whatever is underneath.
  hud.setIgnoreMouseEvents(true);
  hud.on('closed', () => {
    hud = null;
    ready = false;
  });
  // `did-finish-load`, not `ready-to-show`: the window is hidden, and a hidden window may never
  // paint. Same trap as the splash window in main.ts.
  hud.webContents.once('did-finish-load', () => {
    ready = true;
  });
  void hud.loadFile(htmlPath);
  return hud;
}

/**
 * Build the window ahead of first use. Window creation plus the initial page load is tens to
 * hundreds of milliseconds — long enough, on a profile that needs no refresh, for the card to
 * arrive after the browser it was announcing.
 */
export function prewarmTrayHud(): void {
  ensureWindow();
}

function clearTimers(): void {
  for (const timer of [showTimer, hideTimer, dismissTimer]) if (timer) clearTimeout(timer);
  showTimer = null;
  hideTimer = null;
  dismissTimer = null;
}

function paint(tray: Tray, state: 'busy' | 'error', title: string, subtitle: string, gen: number): void {
  const win = ensureWindow();
  if (!win) return;

  const { x, y } = positionFor(tray);
  win.setBounds({ x, y, width: WIDTH, height: HEIGHT });

  const apply = () => {
    if (gen !== generation || !hud || hud.isDestroyed()) return;
    // Values are JSON-encoded into the call and land in textContent on the other side — a subtitle
    // can be an account name or an error string from AWS, neither of which is trusted as code.
    void hud.webContents
      .executeJavaScript(
        `window.__setHudState(${JSON.stringify(state)},${JSON.stringify(title)},${JSON.stringify(subtitle)})`
      )
      .catch(() => {
        // A card that can't render its text is not worth surfacing an error over.
      });
    if (!hud.isVisible()) shownAt = Date.now();
    hud.showInactive();
  };

  if (ready) apply();
  else win.webContents.once('did-finish-load', apply);
}

/** Show the spinner card, after a short delay so quick work never triggers a flicker. */
export function showTrayHudBusy(tray: Tray | null, title: string, subtitle: string): void {
  if (!tray || tray.isDestroyed()) return;
  const gen = ++generation;
  clearTimers();
  // Start the load now even though the paint is deferred, so the delay and the load overlap.
  ensureWindow();

  showTimer = setTimeout(() => {
    if (gen !== generation) return;
    paint(tray, 'busy', title, subtitle, gen);
    dismissTimer = setTimeout(hideTrayHud, MAX_BUSY_MS);
  }, BUSY_SHOW_DELAY_MS);
}

/** Swap the card to an error message, which dismisses itself after a few seconds. */
export function showTrayHudError(tray: Tray | null, title: string, subtitle: string): void {
  if (!tray || tray.isDestroyed()) return;
  const gen = ++generation;
  clearTimers();
  // No show delay: unlike progress, an error is worth showing however briefly the work ran.
  paint(tray, 'error', title, subtitle, gen);
  dismissTimer = setTimeout(hideTrayHud, ERROR_DISMISS_MS);
}

/** Hide, but keep the window around: reuse is what makes the next show instant. */
export function hideTrayHud(): void {
  const gen = ++generation;
  clearTimers();
  if (!hud || hud.isDestroyed() || !hud.isVisible()) return;

  const shownFor = Date.now() - shownAt;
  if (shownFor >= MIN_VISIBLE_MS) {
    hud.hide();
    return;
  }
  hideTimer = setTimeout(() => {
    if (gen !== generation) return;
    if (hud && !hud.isDestroyed()) hud.hide();
  }, MIN_VISIBLE_MS - shownFor);
}

export function destroyTrayHud(): void {
  generation++;
  clearTimers();
  if (hud && !hud.isDestroyed()) hud.destroy();
  hud = null;
  ready = false;
}
