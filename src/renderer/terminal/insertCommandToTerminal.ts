/**
 * Shared helper: insert a command string into the embedded terminal.
 * Writes the text only (no Enter) so you can edit before running.
 */

import type { EmbeddedTerminalRef } from './EmbeddedTerminal';

export function insertCommandToTerminal(terminalRef: EmbeddedTerminalRef | null, command: string): void {
  if (!terminalRef?.write) return;
  const trimmed = command.trim();
  if (!trimmed) return;
  terminalRef.write(trimmed);
  terminalRef.focus?.();
}
