#!/usr/bin/env node
// Fails when src/shared/ipcContract.ts drifts from what main and preload actually use.
// Run: node scripts/check-ipc-contract.mjs
import { readFileSync, readdirSync, statSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const read = (p) => readFileSync(join(root, p), 'utf8');

function walk(dir) {
  const out = [];
  for (const name of readdirSync(join(root, dir))) {
    const rel = join(dir, name);
    if (statSync(join(root, rel)).isDirectory()) out.push(...walk(rel));
    else if (/\.(ts|tsx)$/.test(name)) out.push(rel);
  }
  return out;
}

const matchAll = (text, re) => [...text.matchAll(re)].map((m) => m[1]);

// Contract
const contract = read('src/shared/ipcContract.ts');
const [reqBlock, evtBlock] = contract.split('export const IPC_EVENTS');
const contractRequests = new Set(matchAll(reqBlock, /channel: '([^']+)'/g));
const contractEvents = new Set(matchAll(evtBlock, /channel: '([^']+)'/g));

// Main: handled requests and sent events
const mainFiles = walk('src/main').map(read).join('\n');
const handled = new Set(matchAll(mainFiles, /ipcMain\.handle\(\s*'([^']+)'/g));
const sentLiteral = matchAll(mainFiles, /(?:webContents|sender)\.send\(\s*'([^']+)'/g);
const sentViaBridge = matchAll(mainFiles, /sendToRenderer\(\s*'([^']+)'/g);
// Channels sent through a constant (autoUpdater `channel`, authLogViewer AUTH_AUDIT_UPDATED).
const sentViaConst = matchAll(mainFiles, /const (?:channel|AUTH_AUDIT_UPDATED) = '([^']+)'/g);
const sent = new Set([...sentLiteral, ...sentViaBridge, ...sentViaConst]);

// Preload: invoked and subscribed channels
const preload = read('src/preload/index.ts') + '\n' + read('src/preload/logViewer.ts');
const invoked = new Set(matchAll(preload, /invoke\(\s*'([^']+)'/g));
const subscribed = new Set([
  ...matchAll(preload, /ipcRenderer\.on\(\s*'([^']+)'/g),
  ...matchAll(preload, /const UPDATED = '([^']+)'/g),
]);

const problems = [];
const diff = (a, b, msg) => {
  for (const x of a) if (!b.has(x)) problems.push(`${msg}: ${x}`);
};

diff(handled, contractRequests, 'handled in main but missing from contract');
diff(contractRequests, handled, 'in contract but not handled in main');
diff(invoked, handled, 'invoked by preload but not handled in main');
diff(sent, contractEvents, 'sent by main but missing from contract');
diff(contractEvents, sent, 'in contract but never sent by main');
diff(subscribed, contractEvents, 'subscribed in preload but missing from contract');

if (problems.length) {
  console.error(`IPC contract drift (${problems.length}):\n  ` + problems.join('\n  '));
  process.exit(1);
}
console.log(
  `IPC contract OK: ${contractRequests.size} requests, ${contractEvents.size} events ` +
    `(main handles ${handled.size}, sends ${sent.size}; preload invokes ${invoked.size}, subscribes ${subscribed.size}).`
);
