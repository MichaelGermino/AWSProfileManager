// Writes {"type":"commonjs"} package.json into dist/main and dist/preload after tsc.
// The root package.json declares "type": "module" so Vite 7 (ESM-only) loads correctly,
// but Electron's main and preload bundles are still emitted as CommonJS by tsconfig.main.json.
// These tiny markers tell Node to load the .js files in those directories as CJS, overriding
// the root setting on a per-directory basis.

import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const targets = ['dist/main', 'dist/preload', 'dist/shared'];
const contents = '{"type":"commonjs"}\n';

for (const rel of targets) {
  const dir = path.join(repoRoot, rel);
  if (!fs.existsSync(dir)) {
    console.error(`[write-cjs-markers] ${rel} does not exist; did tsc run?`);
    process.exitCode = 1;
    continue;
  }
  fs.writeFileSync(path.join(dir, 'package.json'), contents);
}
