// Prepare the native modules (keytar, node-pty) for the bundled Electron.
//
//   node scripts/rebuild-native.mjs          full: what `npm run dist` / CI need
//   node scripts/rebuild-native.mjs --host   host arch only: what `postinstall` needs for `npm run dev`
//
// keytar — Windows / Linux / --host: a plain `electron-rebuild --only=keytar` for the host arch,
// exactly as before.
//
// keytar — macOS full: the mac config in package.json builds BOTH x64 and arm64 in one
// electron-builder run, and `npmRebuild` is false, so electron-builder packages whatever
// keytar.node is sitting in node_modules — one file, one arch. Shipping the arm64 build inside the
// x64 app makes keytar fail to load on Intel Macs and every credential read silently returns null.
// So this builds keytar once per arch and `lipo`s the two into a single universal keytar.node that
// loads on either. (Why not `--universal`? It would double the download for a ~200 MB Electron
// app, and @electron/universal would still need per-arch keytar binaries to merge.)
//
// node-pty — never rebuilt (see CLAUDE.md: the Windows source build is broken); it ships NAPI
// prebuilds for every platform and node-gyp-build picks the right one at runtime. But the npm
// tarball drops the execute bit on its macOS `spawn-helper`, the tiny binary node-pty forks to
// set up the pty, and electron-builder copies file modes as they are. Without +x every terminal
// spawn fails with `posix_spawnp failed`, so the bit is restored here on every run.
import { execFileSync } from 'node:child_process';
import { chmodSync, copyFileSync, existsSync, mkdtempSync, readdirSync, rmSync, statSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

const hostOnly = process.argv.includes('--host');
const KEYTAR_NODE = join('node_modules', 'keytar', 'build', 'Release', 'keytar.node');
// Invoked through node rather than `npx electron-rebuild`: `npx` is `npx.cmd` on Windows and
// execFileSync does not resolve that without a shell.
const ELECTRON_REBUILD_CLI = join('node_modules', '@electron', 'rebuild', 'lib', 'cli.js');

function rebuild(arch) {
  const args = [ELECTRON_REBUILD_CLI, '--only=keytar'];
  if (arch) args.push(`--arch=${arch}`);
  console.log(`[rebuild-native] electron-rebuild ${args.slice(1).join(' ')}`);
  execFileSync(process.execPath, args, { stdio: 'inherit' });
}

function fixNodePtyHelperPermissions() {
  const prebuilds = join('node_modules', 'node-pty', 'prebuilds');
  if (process.platform === 'win32' || !existsSync(prebuilds)) return;
  for (const dir of readdirSync(prebuilds)) {
    const helper = join(prebuilds, dir, 'spawn-helper');
    if (!existsSync(helper)) continue;
    const mode = statSync(helper).mode;
    if ((mode & 0o111) === 0o111) continue;
    chmodSync(helper, mode | 0o755);
    console.log(`[rebuild-native] chmod +x ${helper}`);
  }
}

if (process.platform !== 'darwin' || hostOnly) {
  rebuild();
} else {
  const tmp = mkdtempSync(join(tmpdir(), 'keytar-'));
  try {
    const slices = [];
    for (const arch of ['x64', 'arm64']) {
      rebuild(arch);
      const slice = join(tmp, `keytar-${arch}.node`);
      copyFileSync(KEYTAR_NODE, slice);
      slices.push(slice);
    }
    console.log('[rebuild-native] lipo -create -> universal keytar.node');
    execFileSync('lipo', ['-create', ...slices, '-output', KEYTAR_NODE], { stdio: 'inherit' });
    execFileSync('lipo', ['-info', KEYTAR_NODE], { stdio: 'inherit' });
  } finally {
    rmSync(tmp, { recursive: true, force: true });
  }
}
fixNodePtyHelperPermissions();
