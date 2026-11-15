// Checks a generated bridge feed (latest.yml) with electron-updater's own parser, exactly as a 1.x
// client reads it (plan §7.1): the version, a file entry whose sha512 and size match the installer,
// and the download URL it resolves to on the GitHub release.
//
//   node native/tools/check-bridge-feed.mjs native/target/dist/latest.yml <version>
//
// Needs the repository's node_modules (electron-updater, js-yaml): `npm ci --ignore-scripts`.
import { createRequire } from 'node:module';
import { readFileSync } from 'node:fs';
import { createHash } from 'node:crypto';
import { dirname, join } from 'node:path';

const require = createRequire(import.meta.url);
const { parseUpdateInfo, resolveFiles } = require('electron-updater/out/providers/Provider');

const [feedPath, version] = process.argv.slice(2);
if (!feedPath || !version) {
  console.error('usage: check-bridge-feed.mjs <latest.yml> <version>');
  process.exit(2);
}
const text = readFileSync(feedPath, 'utf8');
const base = new URL(`https://github.com/MichaelGermino/AWSProfileManager/releases/download/v${version}/`);
const info = parseUpdateInfo(text, 'latest.yml', new URL('latest.yml', base));
const fail = (msg) => {
  console.error(`bridge feed: ${msg}`);
  process.exit(1);
};
if (info.version !== version) fail(`version ${info.version}, expected ${version}`);
const files = resolveFiles(info, base);
if (files.length !== 1) fail(`${files.length} files, expected 1`);
const file = info.files[0];
const installer = readFileSync(join(dirname(feedPath), file.url));
const sha512 = createHash('sha512').update(installer).digest('base64');
if (file.sha512 !== sha512 || info.sha512 !== sha512) fail('sha512 does not match the installer');
if (file.size !== installer.length) fail(`size ${file.size}, installer is ${installer.length} bytes`);
if (/\s/.test(file.url)) fail('the file name has whitespace');
console.log(`bridge feed OK: ${info.version} -> ${files[0].url.href} (${installer.length} bytes)`);
