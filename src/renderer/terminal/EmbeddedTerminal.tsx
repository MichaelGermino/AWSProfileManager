/**
 * Embedded terminal using xterm.js and node-pty (via IPC).
 * Renders shell I/O with ANSI colors, supports resize, copy/paste, and programmatic write.
 */

import { useEffect, useRef, useState, useCallback } from 'react';
import { Terminal } from '@xterm/xterm';
import { FitAddon } from '@xterm/addon-fit';
import '@xterm/xterm/css/xterm.css';
import { setTerminalApi } from './terminalRefStore';

const TERMINAL_OPTIONS = {
  cursorBlink: true,
  fontSize: 13,
  fontFamily: 'Consolas, "Courier New", monospace',
  theme: {
    background: '#1e1f22',
    foreground: '#f2f3f5',
    cursor: '#f2f3f5',
    cursorAccent: '#1e1f22',
    selectionBackground: 'rgba(88, 101, 242, 0.3)',
    black: '#1e1f22',
    red: '#f23f43',
    green: '#23a559',
    yellow: '#f0b232',
    blue: '#5865f2',
    magenta: '#eb459e',
    cyan: '#00d4aa',
    white: '#f2f3f5',
    brightBlack: '#4e5058',
    brightRed: '#f23f43',
    brightGreen: '#23a559',
    brightYellow: '#f0b232',
    brightBlue: '#5865f2',
    brightMagenta: '#eb459e',
    brightCyan: '#00d4aa',
    brightWhite: '#f2f3f5',
  },
};

export interface EmbeddedTerminalRef {
  /** Write text into the terminal (sends to PTY; appears as if user typed it). */
  write: (data: string) => void;
  /** Focus the terminal so keyboard input works. */
  focus: () => void;
}

interface EmbeddedTerminalProps {
  className?: string;
  /** When true, the terminal tab is visible; when it becomes true we refit so layout/scrollbar are correct after being hidden. */
  isVisible?: boolean;
}

export function EmbeddedTerminal({ className = '', isVisible = true }: EmbeddedTerminalProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const termInstanceRef = useRef<Terminal | null>(null);
  const fitAddonRef = useRef<FitAddon | null>(null);
  const [ptyError, setPtyError] = useState<string | null>(null);

  // Refit and redraw terminal when tab becomes visible again (layout was wrong while hidden)
  useEffect(() => {
    if (!isVisible) return;

    function doRefit() {
      const fitAddon = fitAddonRef.current;
      const term = termInstanceRef.current;
      if (!fitAddon || !term) return;
      fitAddon.fit();
      const dims = fitAddon.proposeDimensions();
      if (dims && typeof window.electron?.terminalResize === 'function') {
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
    window.electron?.terminalStart?.();
  }, []);

  useEffect(() => {
    let cancelled = false;
    let term: Terminal | null = null;
    let removeOnData: (() => void) | undefined;
    let removeOnError: (() => void) | undefined;
    let resizeObserver: ResizeObserver | undefined;

    const frameId = requestAnimationFrame(() => {
      if (cancelled) return;
      const container = containerRef.current;
      if (!container) return;

      term = new Terminal(TERMINAL_OPTIONS);
      const fitAddon = new FitAddon();
      term.loadAddon(fitAddon);
      term.open(container);
      fitAddon.fit();
      termInstanceRef.current = term;
      fitAddonRef.current = fitAddon;

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
        window.electron?.terminalWrite?.(data);
      });

      removeOnData =
        typeof window.electron?.onTerminalData === 'function'
          ? window.electron.onTerminalData((data: string) => term!.write(data))
          : undefined;

      term.focus();

      resizeObserver = new ResizeObserver(() => {
        const dims = fitAddon.proposeDimensions();
        if (!dims || dims.cols <= 0 || dims.rows <= 0) return;
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
      if (term) {
        term.dispose();
      }
      termInstanceRef.current = null;
      fitAddonRef.current = null;
      setTerminalApi(null);
    };
  }, []);

  const handleContainerClick = () => {
    termInstanceRef.current?.focus();
  };

  return (
    <div className={`${className} relative`} style={{ width: '100%', height: '100%', minHeight: 120 }}>
      {ptyError && (
        <div
          className="absolute inset-0 z-10 flex flex-col items-center justify-center gap-3 p-4 bg-discord-darkest/95 text-center"
          role="alert"
        >
          <p className="text-sm text-discord-textMuted">Terminal failed to start</p>
          <p className="text-xs text-discord-danger font-mono max-h-20 overflow-auto">{ptyError}</p>
          <button
            type="button"
            onClick={startPty}
            className="px-4 py-2 rounded-lg bg-discord-accent text-white text-sm font-medium hover:bg-discord-accentHover"
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
