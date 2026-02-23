/**
 * PTY service: spawns and manages node-pty shell processes per renderer window.
 * All terminal I/O stays in the main process; data is streamed to renderer via IPC.
 * On Windows we force winpty (useConpty: false) to avoid "AttachConsole failed" when Electron has no console.
 */

import type { WebContents } from 'electron';

/** One PTY instance per webContents (e.g. one per Terminal tab/window). */
const ptyByWebContentsId = new Map<number, import('node-pty').IPty>();

/** Lazy-loaded node-pty (only when terminal is started) to avoid loading ConPTY path at app startup. */
function getPty(): typeof import('node-pty') {
  return require('node-pty');
}

/**
 * Resolve the shell executable for the current platform.
 * Windows: powershell.exe; Mac/Linux: /bin/bash or /bin/zsh (from SHELL env).
 */
function getShell(): string {
  if (process.platform === 'win32') {
    return 'powershell.exe';
  }
  const shell = process.env.SHELL ?? '/bin/bash';
  return shell;
}

/**
 * Spawn a new PTY shell and associate it with the given webContents.
 * Output is streamed to the renderer via 'terminal:data'.
 * On failure sends 'terminal:error' with the message.
 */
export function startTerminal(webContents: WebContents): void {
  const id = webContents.id;
  disposeTerminal(webContents);

  const shell = getShell();
  const cols = 80;
  const rows = 24;
  const cwd = process.env.HOME || process.env.USERPROFILE || process.cwd();
  const env = { ...process.env };

  const spawnOptions: Record<string, unknown> = {
    name: 'xterm-256color',
    cols,
    rows,
    cwd: cwd || undefined,
    env,
  };
  if (process.platform === 'win32') {
    (spawnOptions as import('node-pty').IWindowsPtyForkOptions).useConpty = false;
  }

  try {
    const pty = getPty();
    const ptyProcess = pty.spawn(shell, [], spawnOptions as import('node-pty').IPtyForkOptions);

    ptyProcess.onData((data: string) => {
      if (!webContents.isDestroyed()) {
        webContents.send('terminal:data', data);
      }
    });

    ptyProcess.onExit(() => {
      ptyByWebContentsId.delete(id);
    });

    ptyByWebContentsId.set(id, ptyProcess);
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
 */
export function writeToTerminal(webContents: WebContents, data: string): void {
  const ptyProcess = ptyByWebContentsId.get(webContents.id);
  if (ptyProcess) {
    ptyProcess.write(data);
  }
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
  const ptyProcess = ptyByWebContentsId.get(webContents.id);
  if (ptyProcess) {
    ptyProcess.kill();
    ptyByWebContentsId.delete(webContents.id);
  }
}
