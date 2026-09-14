import { useEffect, useState } from 'react';
import type { Settings } from '../shared/types';

/**
 * Resolves the animated background: where it applies, and which video to play.
 *
 * Shared by the sign-in screen and the app shell so the two can never disagree about whether the
 * background is on.
 *
 * FIRST-PAINT CACHE — settings.json is the source of truth, but reading it is an IPC round trip
 * that resolves a frame or two after mount. On the sign-in screen, which is the very first thing
 * drawn, that means the background visibly appears (or the wrong one appears) after the fact. So
 * the resolved config is mirrored into localStorage and read back synchronously at boot. The
 * mirror is only ever a cache: settings.json still wins the moment it arrives.
 */

export type BackgroundScope = 'off' | 'auth' | 'app';

export interface BackgroundConfig {
  scope: BackgroundScope;
  /** Playable URL, or null when neither a custom nor a bundled video exists. */
  videoUrl: string | null;
  /** Scope to return to when switching back on; never 'off'. */
  lastEnabledScope: Exclude<BackgroundScope, 'off'>;
  /** Backdrop blur in px behind app surfaces in 'app' scope. */
  blur: number;
}

/** Low enough that the video still visibly moves, high enough that text stays comfortable. */
export const DEFAULT_BACKGROUND_BLUR = 12;
export const MIN_BACKGROUND_BLUR = 0;
export const MAX_BACKGROUND_BLUR = 40;

function clampBlur(value: unknown): number {
  const n = typeof value === 'number' && Number.isFinite(value) ? value : DEFAULT_BACKGROUND_BLUR;
  return Math.min(MAX_BACKGROUND_BLUR, Math.max(MIN_BACKGROUND_BLUR, Math.round(n)));
}

/**
 * Video shipped with the app, if one was placed in src/renderer/assets. A glob rather than a plain
 * import because a static import of a missing file is a build error — this asset is optional.
 */
const bundledModules = import.meta.glob('./assets/auth-background.{mp4,webm}', {
  eager: true,
  query: '?url',
  import: 'default',
});
const bundledVideoUrl = (Object.values(bundledModules)[0] as string | undefined) ?? null;

const CACHE_KEY = 'background-config';
/** Fired after Settings changes anything, so open screens re-resolve without a reload. */
export const BACKGROUND_CHANGED_EVENT = 'background:changed';

const DEFAULT_CONFIG: BackgroundConfig = {
  // 'auth' matches how the feature first shipped, so an upgrade changes nothing on its own.
  scope: 'auth',
  videoUrl: bundledVideoUrl,
  lastEnabledScope: 'auth',
  blur: DEFAULT_BACKGROUND_BLUR,
};

export function readCachedConfig(): BackgroundConfig {
  try {
    const raw = localStorage.getItem(CACHE_KEY);
    if (!raw) return DEFAULT_CONFIG;
    const parsed = JSON.parse(raw) as Partial<BackgroundConfig>;
    return {
      scope: parsed.scope ?? DEFAULT_CONFIG.scope,
      // A cached custom URL is trusted; only fall back to the bundled one when none was cached.
      videoUrl: parsed.videoUrl ?? bundledVideoUrl,
      lastEnabledScope: parsed.lastEnabledScope ?? 'auth',
      blur: clampBlur(parsed.blur),
    };
  } catch {
    return DEFAULT_CONFIG;
  }
}

function writeCachedConfig(config: BackgroundConfig): void {
  try {
    localStorage.setItem(CACHE_KEY, JSON.stringify(config));
  } catch {
    // Cosmetic cache only — never break rendering over it.
  }
}

/** Custom video wins over the bundled one; the timestamp busts Chromium's cache on replacement. */
export function resolveConfig(settings: Settings | null | undefined): BackgroundConfig {
  const scope = settings?.backgroundScope ?? 'auth';
  const custom = settings?.backgroundVideoPath
    ? `appmedia://video/${settings.backgroundVideoUpdatedAt ?? 0}`
    : null;
  const previous = readCachedConfig();
  return {
    scope,
    videoUrl: custom ?? bundledVideoUrl,
    lastEnabledScope: scope === 'off' ? previous.lastEnabledScope : scope,
    blur: clampBlur(settings?.backgroundBlur),
  };
}

/**
 * Videos that failed to load, by URL.
 *
 * settings.json records the path but cannot guarantee the file is still there — cleared app data,
 * antivirus, or a manual tidy-up all leave a path pointing at nothing. Checking from the renderer
 * would need another IPC round trip on the first screen painted, and would still not catch a file
 * that exists but will not decode.
 *
 * So the <video> element's own `error` event is the signal: whatever the cause, the URL is marked
 * unusable and every consumer immediately behaves as though no video were configured — the plain
 * background, and no toggle button offering to switch to something that cannot appear.
 *
 * Choosing a new video produces a new URL (the timestamp changes), so it is never suppressed by an
 * earlier failure.
 */
const brokenVideoUrls = new Set<string>();
const brokenVideoListeners = new Set<() => void>();

export function markVideoUnavailable(url: string): void {
  if (brokenVideoUrls.has(url)) return;
  brokenVideoUrls.add(url);
  brokenVideoListeners.forEach((listener) => listener());
}

/**
 * Blur is applied as a CSS variable on <html> rather than passed down as a prop.
 *
 * Two reasons: the glass rules in index.css target surfaces all over the tree (main, the sidebar,
 * every panel), so a prop would have to reach all of them; and Settings can write the variable
 * directly while dragging the slider, giving live feedback without a settings write per pixel.
 */
export function applyBlurVariable(blurPx: number): void {
  document.documentElement.style.setProperty('--glass-blur', `${clampBlur(blurPx)}px`);
}

interface BackgroundElectron {
  getSettings: () => Promise<Settings>;
  saveSettings: (settings: Settings) => Promise<void>;
}

/** Persist a new scope, updating the cache immediately so the UI does not wait on the round trip. */
export async function setBackgroundScope(scope: BackgroundScope): Promise<void> {
  const electron = window.electron as unknown as BackgroundElectron;
  const settings = await electron.getSettings();
  await electron.saveSettings({ ...settings, backgroundScope: scope });
  writeCachedConfig(resolveConfig({ ...settings, backgroundScope: scope }));
  window.dispatchEvent(new Event(BACKGROUND_CHANGED_EVENT));
}

/**
 * Current config: the cached value immediately, replaced by settings.json once it loads, and
 * re-resolved whenever Settings announces a change.
 */
export function useBackgroundConfig(): BackgroundConfig {
  const [config, setConfig] = useState<BackgroundConfig>(readCachedConfig);
  // Bumped when a video is found to be unplayable, purely to force a re-read of the broken set.
  const [, setBrokenVersion] = useState(0);

  useEffect(() => {
    let cancelled = false;
    const refresh = async () => {
      try {
        const electron = window.electron as unknown as BackgroundElectron;
        const settings = await electron.getSettings();
        if (cancelled) return;
        const resolved = resolveConfig(settings);
        writeCachedConfig(resolved);
        setConfig(resolved);
      } catch {
        // Keep whatever the cache gave us.
      }
    };
    void refresh();
    window.addEventListener(BACKGROUND_CHANGED_EVENT, refresh);

    const onBroken = () => setBrokenVersion((v) => v + 1);
    brokenVideoListeners.add(onBroken);

    return () => {
      cancelled = true;
      window.removeEventListener(BACKGROUND_CHANGED_EVENT, refresh);
      brokenVideoListeners.delete(onBroken);
    };
  }, []);

  // A video that failed to load is reported as no video at all, so callers hide the toggle and
  // fall back to the plain background rather than offering something that cannot be shown.
  if (config.videoUrl && brokenVideoUrls.has(config.videoUrl)) {
    return { ...config, videoUrl: null };
  }
  return config;
}
