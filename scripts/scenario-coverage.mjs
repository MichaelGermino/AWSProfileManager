#!/usr/bin/env node
// Which IPC channels and events (src/shared/ipcContract.ts) no backend scenario exercises yet.
// Run: node scripts/scenario-coverage.mjs [--strict]   (--strict exits 1 while anything is uncovered)
// A channel can be marked as covered elsewhere (UI flow tests) in UI_ONLY below, with a reason.
import { readFileSync, readdirSync, statSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const contract = readFileSync(join(root, 'src/shared/ipcContract.ts'), 'utf8');
const [reqBlock, evtBlock] = contract.split('export const IPC_EVENTS');
const requests = [...reqBlock.matchAll(/channel: '([^']+)'/g)].map((m) => m[1]);
const events = [...evtBlock.split('export const NON_IPC_SURFACE')[0].matchAll(/channel: '([^']+)'/g)].map((m) => m[1]);

/** Channels that only make sense through the UI or a packaged build; covered by UI flows instead. */
const UI_ONLY = {
  'window:minimize': 'window chrome — UI flow',
  'window:maximize': 'window chrome — UI flow',
  'window:close': 'hide-to-tray — UI flow',
  openDevTools: 'debug only — UI flow',
  'logs:openAuthViewer': 'opens a window — UI flow',
  'update:installAndRestart': 'packaged builds only — manual/upgrade test',
  'update:checkNow': 'packaged builds only — manual/upgrade test',
  'update:getStatus': 'packaged builds only — manual/upgrade test',
  update: 'packaged builds only',
  'logs:authAuditUpdated': 'log viewer window live refresh — UI flow',
};

function walk(dir) {
  return readdirSync(dir).flatMap((n) => {
    const p = join(dir, n);
    return statSync(p).isDirectory() ? walk(p) : p.endsWith('.json') ? [p] : [];
  });
}
const text = walk(join(root, 'native/tests/fixtures/scenarios')).map((f) => readFileSync(f, 'utf8')).join('\n');
const called = new Set([...text.matchAll(/"call":\s*"([^"]+)"/g)].map((m) => m[1]));
// Channels behind one composed preload method (the AWS CLI docs flow) are exercised together.
const methodOf = new Map([...reqBlock.matchAll(/channel: '([^']+)', method: (?:'([^']+)'|null)/g)].map((m) => [m[1], m[2] ?? null]));
for (const [channel, method] of methodOf) {
  if (method && [...called].some((c) => methodOf.get(c) === method)) called.add(channel);
}
const seenEvents = new Set([
  ...[...text.matchAll(/\[\s*"([a-zA-Z]+:[a-zA-Z-]+|notify|update)"/g)].map((m) => m[1]),
  ...[...text.matchAll(/"(?:expectStream|waitForStream)":\s*"([^"]+)"/g)].map((m) => m[1]),
]);

const missReq = requests.filter((c) => !called.has(c) && !(c in UI_ONLY));
const missEvt = events.filter((c) => !seenEvents.has(c) && !(c in UI_ONLY));
const pct = (n, d) => `${Math.round((100 * n) / d)}%`;
console.log(`requests: ${requests.length - missReq.length}/${requests.length} covered (${pct(requests.length - missReq.length, requests.length)}), ${Object.keys(UI_ONLY).filter((c) => requests.includes(c)).length} UI-only`);
console.log(`events:   ${events.length - missEvt.length}/${events.length} covered`);
if (missReq.length) console.log(`\nrequests without a scenario:\n  ${missReq.join('\n  ')}`);
if (missEvt.length) console.log(`\nevents never asserted:\n  ${missEvt.join('\n  ')}`);
if (process.argv.includes('--strict') && (missReq.length || missEvt.length)) process.exit(1);
