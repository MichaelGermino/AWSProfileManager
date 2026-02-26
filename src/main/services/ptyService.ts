/**
 * PTY service: spawns and manages node-pty shell processes per renderer window.
 * All terminal I/O stays in the main process; data is streamed to renderer via IPC.
 * On Windows we use winpty (useConpty: false) for both PowerShell and Git Bash
 * so keyboard input works and to avoid "AttachConsole failed".
 */

import type { WebContents } from 'electron';
import { getSettings } from './settingsService';

/** One PTY instance per webContents (e.g. one per Terminal tab/window). */
const ptyByWebContentsId = new Map<number, import('node-pty').IPty>();
/** Shell type per webContents so we can normalize input for Git Bash on Windows. */
const shellTypeByWebContentsId = new Map<number, TerminalShellType>();
/** Last time we wrote an Enter (\\r only) for Bash; throttle duplicate Enters. */
const lastBashEnterByWebContentsId = new Map<number, number>();

const BASH_ENTER_THROTTLE_MS = 600;

/** Lazy-loaded node-pty (only when terminal is started) to avoid loading ConPTY path at app startup. */
function getPty(): typeof import('node-pty') {
  return require('node-pty');
}

export type TerminalShellType = 'powershell' | 'bash';

/**
 * Resolve the shell executable and args for the current platform.
 * On Windows when shell is 'bash': use settings.bashPath (Git's bin\bash.exe); fallback to powershell if empty.
 * On non-Windows: use SHELL or /bin/bash with -i.
 */
function getShellAndArgs(shell: TerminalShellType): { shell: string; args: string[] } {
  if (shell === 'bash') {
    if (process.platform === 'win32') {
      const bashPath = (getSettings().bashPath ?? '').trim();
      if (bashPath) return { shell: bashPath, args: ['--login', '-i'] };
      return { shell: 'powershell.exe', args: [] };
    }
    return { shell: process.env.SHELL ?? '/bin/bash', args: ['-i'] };
  }
  if (process.platform === 'win32') {
    return { shell: 'powershell.exe', args: [] };
  }
  return { shell: process.env.SHELL ?? '/bin/bash', args: [] };
}

/**
 * Spawn a new PTY shell and associate it with the given webContents.
 * Output is streamed to the renderer via 'terminal:data'.
 * On failure sends 'terminal:error' with the message.
 * @param options.shell - 'powershell' or 'bash'. Bash on Windows uses settings.bashPath (Git Bash).
 */
export function startTerminal(webContents: WebContents, options?: { shell: TerminalShellType }): void {
  const id = webContents.id;
  disposeTerminal(webContents);

  const shellType = options?.shell ?? getSettings().terminalShell ?? 'powershell';
  const { shell, args } = getShellAndArgs(shellType);
  const cols = 80;
  const rows = 24;
  const cwd = process.env.HOME || process.env.USERPROFILE || process.cwd();
  const env = { ...process.env };
  if (process.platform === 'win32' && shellType === 'bash') {
    env.MSYSTEM = 'MINGW64';
  }

  const spawnOptions: Record<string, unknown> = {
    name: 'xterm-256color',
    cols,
    rows,
    cwd: cwd || undefined,
    env,
  };
  if (process.platform === 'win32') {
    const winOpts = spawnOptions as import('node-pty').IWindowsPtyForkOptions;
    winOpts.useConpty = false;
  }

  try {
    const pty = getPty();
    const ptyProcess = pty.spawn(shell, args, spawnOptions as import('node-pty').IPtyForkOptions);

    ptyProcess.onData((data: string) => {
      if (!webContents.isDestroyed()) {
        webContents.send('terminal:data', data);
      }
    });

    ptyProcess.onExit(() => {
      // Only clear maps if this process is still the one registered (avoids old PTY's onExit wiping the new one after shell switch)
      if (ptyByWebContentsId.get(id) === ptyProcess) {
        ptyByWebContentsId.delete(id);
        shellTypeByWebContentsId.delete(id);
        lastBashEnterByWebContentsId.delete(id);
      }
    });

    ptyByWebContentsId.set(id, ptyProcess);
    shellTypeByWebContentsId.set(id, shellType);
  } catch (err) {
    const message = err instanceof Error ? err.message : String(err);
    if (!webContents.isDestroyed()) {
      webContents.send('terminal:error', message);
    }
    console.error('[ptyService] startTerminal failed:', err);
  }
}

/**
 * Write input to the PTY associated with this webContents.
 * For Git Bash on Windows with winpty, the shell expects \r (carriage return) for Enter;
 * normalize any \r\n or \n to single \r so submitted lines execute.
 */
export function writeToTerminal(webContents: WebContents, data: string): void {
  const ptyProcess = ptyByWebContentsId.get(webContents.id);
  if (!ptyProcess) return;
  const id = webContents.id;
  let out = data;
  if (process.platform === 'win32' && shellTypeByWebContentsId.get(id) === 'bash') {
    out = data.replace(/\r\n|\n/g, '\r').replace(/\r+/g, '\r');
    if (out === '\r') {
      const now = Date.now();
      if (now - (lastBashEnterByWebContentsId.get(id) ?? 0) < BASH_ENTER_THROTTLE_MS) return;
      lastBashEnterByWebContentsId.set(id, now);
    }
  }
  ptyProcess.write(out);
}

/**
 * Resize the PTY associated with this webContents.
 */
export function resizeTerminal(webContents: WebContents, cols: number, rows: number): void {
  const ptyProcess = ptyByWebContentsId.get(webContents.id);
  if (ptyProcess) {
    try {
      ptyProcess.resize(cols, rows);
    } catch {
      // ignore resize errors (e.g. if pty already exited)
    }
  }
}

/**
 * Kill the PTY for this webContents and remove it from the map.
 */
export function disposeTerminal(webContents: WebContents): void {
  const id = webContents.id;
  const ptyProcess = ptyByWebContentsId.get(id);
  if (ptyProcess) {
    ptyProcess.kill();
    ptyByWebContentsId.delete(id);
    shellTypeByWebContentsId.delete(id);
    lastBashEnterByWebContentsId.delete(id);
  }
}
