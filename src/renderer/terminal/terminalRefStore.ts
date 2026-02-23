/**
 * Store for the embedded terminal API so Insert and write work regardless of React ref timing.
 * Set when the terminal mounts, cleared when it unmounts.
 */

import type { EmbeddedTerminalRef } from './EmbeddedTerminal';

let current: EmbeddedTerminalRef | null = null;

export function setTerminalApi(api: EmbeddedTerminalRef | null): void {
  current = api;
}

export function getTerminalApi(): EmbeddedTerminalRef | null {
  return current;
}
