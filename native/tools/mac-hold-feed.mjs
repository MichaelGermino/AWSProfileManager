// Writes the macOS feed for a 2.x release that has no Mac build yet (2.0 is Windows-only until the
// Mac port): it keeps 1.x on a Mac on the last 1.x Mac release instead of failing its update check.
//
// 1.x's electron-updater on a Mac reads latest-mac.yml (or <channel>-mac.yml, for pre-release
// installs) from the newest release. Without one there, every check ends in an update error. The
// feed written here is the last Mac release's own feed with each file name made into a full URL on
// that release, so the 2.x release can carry it: the version it names is the one installed, and
// electron-updater reports no update.
//
//   node native/tools/mac-hold-feed.mjs <source latest-mac.yml> <source tag> <release version> <out dir>
//
// Writes latest-mac.yml, plus <channel>-mac.yml when <release version> is a pre-release
// (2.0.0-beta.4 -> beta-mac.yml), then reads each back with electron-updater's own parser.
// Needs the repository's node_modules (electron-updater, js-yaml): `npm ci --ignore-scripts`.
import { createRequire } from 'node:module';
import { readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

const require = createRequire(import.meta.url);
const yaml = require('js-yaml');
const { parseUpdateInfo, resolveFiles } = require('electron-updater/out/providers/Provider');

const [sourcePath, sourceTag, version, outDir] = process.argv.slice(2);
if (!sourcePath || !sourceTag || !version || !outDir) {
  console.error('usage: mac-hold-feed.mjs <source latest-mac.yml> <source tag> <release version> <out dir>');
  process.exit(2);
}
const fail = (msg) => {
  console.error(`mac hold feed: ${msg}`);
  process.exit(1);
};

const repo = 'https://github.com/MichaelGermino/AWSProfileManager';
const sourceBase = `${repo}/releases/download/${sourceTag}/`;
const feed = yaml.load(readFileSync(sourcePath, 'utf8'));
if (!feed || typeof feed.version !== 'string' || !Array.isArray(feed.files) || feed.files.length === 0) {
  fail('the source feed has no version or files');
}
if (`v${feed.version}` !== sourceTag) fail(`the source feed is ${feed.version}, its tag is ${sourceTag}`);
const absolute = (name) => (/^https?:\/\//.test(name) ? name : sourceBase + encodeURIComponent(name).replace(/%2F/g, '/'));
feed.files = feed.files.map((f) => ({ ...f, url: absolute(f.url) }));
if (typeof feed.path === 'string') feed.path = absolute(feed.path);
const text = yaml.dump(feed, { lineWidth: -1 });

// 2.0.0-beta.4 -> "beta": the channel file electron-updater asks for on a pre-release.
const channel = /-([a-z]+)/i.exec(version)?.[1]?.toLowerCase();
const names = ['latest-mac.yml', ...(channel && channel !== 'latest' ? [`${channel}-mac.yml`] : [])];

// Check before writing: parsed as a client on the 2.x release would, every file must resolve to
// the source release, at the version the source names.
const releaseBase = new URL(`${repo}/releases/download/v${version}/`);
for (const name of names) {
  const info = parseUpdateInfo(text, name, new URL(name, releaseBase));
  if (info.version !== feed.version) fail(`${name}: version ${info.version}, expected ${feed.version}`);
  for (const f of resolveFiles(info, releaseBase)) {
    if (!f.url.href.startsWith(sourceBase)) fail(`${name}: ${f.url.href} is not on ${sourceTag}`);
  }
}
for (const name of names) {
  writeFileSync(join(outDir, name), text);
  console.log(`mac hold feed OK: ${name} -> ${feed.version} (${feed.files.length} files on ${sourceTag})`);
}
