/**
 * Light / dark theme. `settings.theme` is 'system' (default), 'light' or 'dark'; the resolved
 * palette is written to <html data-theme> (colour values in theme.css). 'system' follows the OS
 * and updates live when the OS switches.
 */
export type ThemePreference = 'system' | 'light' | 'dark';

const media = typeof window !== 'undefined' ? window.matchMedia('(prefers-color-scheme: light)') : null;
let current: ThemePreference = 'system';
const listeners = new Set<(theme: 'light' | 'dark') => void>();

function resolve(pref: ThemePreference): 'light' | 'dark' {
  if (pref === 'light' || pref === 'dark') return pref;
  return media?.matches ? 'light' : 'dark';
}

function apply(): void {
  const next = resolve(current);
  if (document.documentElement.dataset.theme === next) return;
  document.documentElement.dataset.theme = next;
  for (const cb of listeners) cb(next);
}

media?.addEventListener('change', () => {
  if (current === 'system') apply();
});

export function applyTheme(pref: ThemePreference | undefined): void {
  current = pref === 'light' || pref === 'dark' ? pref : 'system';
  apply();
}

/** The palette in effect right now (for things drawn outside CSS, like the terminal). */
export function resolvedTheme(): 'light' | 'dark' {
  return resolve(current);
}

/** Called with the new palette whenever it changes. Returns an unsubscribe function. */
export function onThemeChange(cb: (theme: 'light' | 'dark') => void): () => void {
  listeners.add(cb);
  return () => listeners.delete(cb);
}
