/**
 * Embedded terminal using xterm.js and node-pty (via IPC).
 * Renders shell I/O with ANSI colors, supports resize, copy/paste, and programmatic write.
 */

import { useEffect, useRef, useState, useCallback } from 'react';
import { Terminal } from '@xterm/xterm';
import { FitAddon } from '@xterm/addon-fit';
import '@xterm/xterm/css/xterm.css';
import { setTerminalApi } from './terminalRefStore';
import { onThemeChange, resolvedTheme } from '../theme';

/** Terminal palettes, matched to theme.css (page/sunken surface, Plex Mono, semantic hues). */
const TERMINAL_THEMES = {
  dark: {
    background: '#111112',
    foreground: '#e4e4e7',
    cursor: '#e4e4e7',
    cursorAccent: '#111112',
    selectionBackground: 'rgba(63, 179, 166, 0.28)',
    black: '#27272a',
    red: '#e5484d',
    green: '#4cc38a',
    yellow: '#f0b232',
    blue: '#6e9ef5',
    magenta: '#c084fc',
    cyan: '#3fb3a6',
    white: '#d4d4d8',
    brightBlack: '#71717a',
    brightRed: '#ff6b70',
    brightGreen: '#6ee7a8',
    brightYellow: '#fcd34d',
    brightBlue: '#93b8ff',
    brightMagenta: '#d8b4fe',
    brightCyan: '#5fd4c6',
    brightWhite: '#fafafa',
  },
  light: {
    background: '#ffffff',
    foreground: '#27272a',
    cursor: '#27272a',
    cursorAccent: '#ffffff',
    selectionBackground: 'rgba(15, 118, 110, 0.18)',
    black: '#27272a',
    red: '#c81e1e',
    green: '#15803d',
    yellow: '#a16207',
    blue: '#1d4ed8',
    magenta: '#9333ea',
    cyan: '#0f766e',
    white: '#71717a',
    brightBlack: '#52525b',
    brightRed: '#dc2626',
    brightGreen: '#16a34a',
    brightYellow: '#b45309',
    brightBlue: '#2563eb',
    brightMagenta: '#a855f7',
    brightCyan: '#0d9488',
    brightWhite: '#18181b',
  },
};

const TERMINAL_OPTIONS = {
  cursorBlink: true,
  fontSize: 13,
  lineHeight: 1.2,
  fontFamily: '"IBM Plex Mono", Consolas, "Courier New", monospace',
};

export interface EmbeddedTerminalRef {
  /** Write text into the terminal (sends to PTY; appears as if user typed it). */
  write: (data: string) => void;
  /** Focus the terminal so keyboard input works. */
  focus: () => void;
}

export type TerminalShell = 'powershell' | 'bash';

interface EmbeddedTerminalProps {
  className?: string;
  /** When true, the terminal tab is visible; when it becomes true we refit so layout/scrollbar are correct after being hidden. */
  isVisible?: boolean;
  /** Shell to start: powershell or bash. Default powershell. */
  shell?: TerminalShell;
}

const BASH_ENTER_SEND_COOLDOWN_MS = 400;

/** Below this the "size" is a hidden/collapsed container, not a real user resize. */
const MIN_SANE_COLS = 20;
const MIN_SANE_ROWS = 4;

/** True when the element is actually laid out (not display:none and has real size). */
function hasLayout(el: HTMLElement): boolean {
  return el.offsetParent !== null && el.clientWidth > 50 && el.clientHeight > 50;
}

export function EmbeddedTerminal({ className = '', isVisible = true, shell = 'powershell' }: EmbeddedTerminalProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const termInstanceRef = useRef<Terminal | null>(null);
  const fitAddonRef = useRef<FitAddon | null>(null);
  const lastBashEnterSentRef = useRef(0);
  const [ptyError, setPtyError] = useState<string | null>(null);

  // Refit and redraw terminal when tab becomes visible again (layout was wrong while hidden)
  useEffect(() => {
    if (!isVisible) return;

    function doRefit() {
      const fitAddon = fitAddonRef.current;
      const term = termInstanceRef.current;
      if (!fitAddon || !term) return;
      // The early retries can fire before layout has actually happened; fitting a
      // zero-size container would shrink the PTY and rewrap scrollback (see the
      // ResizeObserver note below). The later retries cover it once layout is in.
      const container = containerRef.current;
      if (!container || !hasLayout(container)) return;
      const dims = fitAddon.proposeDimensions();
      if (!dims || !Number.isFinite(dims.cols) || !Number.isFinite(dims.rows)) return;
      if (dims.cols < MIN_SANE_COLS || dims.rows < MIN_SANE_ROWS) return;
      fitAddon.fit();
      if (typeof window.electron?.terminalResize === 'function') {
        window.electron.terminalResize(dims.cols, dims.rows);
      }
      term.refresh(0, term.rows - 1);
    }

    const id1 = requestAnimationFrame(() => {
      requestAnimationFrame(doRefit);
    });
    const id2 = window.setTimeout(doRefit, 150);
    const id3 = window.setTimeout(doRefit, 400);
    return () => {
      cancelAnimationFrame(id1);
      window.clearTimeout(id2);
      window.clearTimeout(id3);
    };
  }, [isVisible]);

  const startPty = useCallback(() => {
    setPtyError(null);
    window.electron?.terminalStart?.({ shell });
  }, [shell]);

  useEffect(() => {
    let cancelled = false;
    let term: Terminal | null = null;
    let removeOnData: (() => void) | undefined;
    let removeOnError: (() => void) | undefined;
    let removeOnTheme: (() => void) | undefined;
    let resizeObserver: ResizeObserver | undefined;

    const frameId = requestAnimationFrame(() => {
      if (cancelled) return;
      const container = containerRef.current;
      if (!container) return;

      term = new Terminal({ ...TERMINAL_OPTIONS, theme: TERMINAL_THEMES[resolvedTheme()] });
      const themed = term;
      removeOnTheme = onThemeChange((t) => {
        themed.options.theme = TERMINAL_THEMES[t];
      });
      const fitAddon = new FitAddon();
      term.loadAddon(fitAddon);
      term.open(container);
      // All screens mount at app start; if the app opened on another screen this
      // container is display:none and fitting now would size the terminal to nothing.
      // The isVisible refit effect handles the first real fit in that case.
      if (hasLayout(container)) fitAddon.fit();
      termInstanceRef.current = term;
      fitAddonRef.current = fitAddon;
      // xterm measures the cell size once. If Plex Mono was not loaded yet it measured the
      // fallback, so re-apply the font when it arrives and refit.
      void document.fonts?.load(`${TERMINAL_OPTIONS.fontSize}px "IBM Plex Mono"`).then(() => {
        if (cancelled || !themed.element) return;
        themed.options.fontFamily = TERMINAL_OPTIONS.fontFamily;
        if (hasLayout(container)) fitAddon.fit();
      });

      const isBashOnWindows = shell === 'bash' && (window as { electron?: { platform?: string } }).electron?.platform === 'win32';

      const api: EmbeddedTerminalRef = {
        write(data: string) {
          window.electron?.terminalWrite?.(data);
        },
        focus() {
          term?.focus();
        },
      };
      setTerminalApi(api);

      startPty();

      removeOnError =
        typeof window.electron?.onTerminalError === 'function'
          ? window.electron.onTerminalError((message: string) => setPtyError(message))
          : undefined;

      term.onData((data: string) => {
        if (isBashOnWindows && data === '\r') return; // Enter handled only in onKey
        window.electron?.terminalWrite?.(data);
      });

      term.onKey((e) => {
        const ev = e.domEvent;
        const ctrl = ev.ctrlKey || ev.metaKey;

        // Only handle copy/paste for Bash; PowerShell uses default behavior to avoid double paste
        if (isBashOnWindows) {
          if (ctrl && ev.key === 'c') {
            if (term.hasSelection()) {
              ev.preventDefault();
              ev.stopImmediatePropagation();
              const text = term.getSelection();
              if (text) {
                void navigator.clipboard.writeText(text).catch(() => {
                  document.execCommand('copy');
                });
              }
            }
            return;
          }
          if (ctrl && ev.key === 'v') {
            ev.preventDefault();
            ev.stopImmediatePropagation();
            navigator.clipboard
              .readText()
              .then((text) => {
                if (text && typeof window.electron?.terminalWrite === 'function') {
                  window.electron.terminalWrite(text);
                }
              })
              .catch(() => {});
            return;
          }
        }

        if (isBashOnWindows && (ev.key === 'Enter' || e.key === '\r') && !ev.repeat) {
          ev.preventDefault();
          ev.stopImmediatePropagation();
          const now = Date.now();
          if (now - lastBashEnterSentRef.current < BASH_ENTER_SEND_COOLDOWN_MS) return;
          lastBashEnterSentRef.current = now;
          window.electron?.terminalWrite?.('\r\n');
        }
      });

      removeOnData =
        typeof window.electron?.onTerminalData === 'function'
          ? window.electron.onTerminalData((data: string) => term!.write(data))
          : undefined;

      term.focus();

      resizeObserver = new ResizeObserver(() => {
        // The Terminal screen stays mounted and is hidden via CSS when another screen is
        // shown, so this observer also fires as the container collapses to nothing. A fit
        // at that moment measures a tiny box (~12 cols) and resizing the PTY that small
        // makes ConPTY rewrap the whole scrollback — which is not undone by sizing back
        // up, leaving history permanently scrunched. Only fit while actually laid out.
        if (!hasLayout(container)) return;
        const dims = fitAddon.proposeDimensions();
        if (!dims || !Number.isFinite(dims.cols) || !Number.isFinite(dims.rows)) return;
        if (dims.cols < MIN_SANE_COLS || dims.rows < MIN_SANE_ROWS) return;
        fitAddon.fit();
        if (typeof window.electron?.terminalResize === 'function') {
          window.electron.terminalResize(dims.cols, dims.rows);
        }
      });
      resizeObserver.observe(container);
    });

    return () => {
      cancelled = true;
      cancelAnimationFrame(frameId);
      resizeObserver?.disconnect();
      removeOnData?.();
      removeOnError?.();
      removeOnTheme?.();
      if (term) {
        term.dispose();
      }
      termInstanceRef.current = null;
      fitAddonRef.current = null;
      setTerminalApi(null);
    };
  }, [shell]);

  const handleContainerClick = () => {
    termInstanceRef.current?.focus();
  };

  return (
    <div className={`${className} relative`} style={{ width: '100%', height: '100%', minHeight: 120 }}>
      {ptyError && (
        <div
          className="absolute inset-0 z-10 flex flex-col items-center justify-center gap-3 p-4 bg-ui-sunken/95 text-center"
          role="alert"
        >
          <p className="text-sm text-ui-textMuted">Terminal failed to start</p>
          <p className="text-xs text-ui-danger font-mono max-h-20 overflow-auto">{ptyError}</p>
          <button
            type="button"
            onClick={startPty}
            className="btn-primary"
          >
            Retry
          </button>
        </div>
      )}
      <div
        ref={containerRef}
        style={{ width: '100%', height: '100%', cursor: 'text' }}
        role="application"
        aria-label="AWS CLI terminal"
        onClick={handleContainerClick}
      />
    </div>
  );
}
