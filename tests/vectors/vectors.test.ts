/**
 * Layer A (docs/native-rewrite-plan.md §5.1): pure-function vectors shared with the Rust port.
 * Each file in native/tests/fixtures/vectors/ is { "function": name, "cases": [{ name, args, expect }] }.
 * The Rust side (cargo test -p apm-core --test vectors) runs the same files against its ports.
 *
 * Run: npx vitest run --config tests/vectors/vitest.config.ts
 * Record expectations for cases whose "expect" is "$record": VECTORS_RECORD=1 npx vitest run ...
 */
import crypto from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { describe, expect, it } from 'vitest';
import { validateMasterPassword } from '../../src/shared/masterPassword';
import { applyOrgConfigToSettings, describeOrgConfig, isOrgConfigEmpty, parseOrgConfig } from '../../src/shared/orgConfig';
import { flattenGrouping, groupProfilesByFolder } from '../../src/shared/profileGrouping';
import { distinctOrgs, normalizeStartUrl, orgKey, orgOfProfile, resolveAuthType, sameOrg } from '../../src/shared/ssoOrg';
import { resolveTerminalProfileId } from '../../src/shared/terminalProfile';
import { classifyAuthFailure } from '../../src/main/services/networkErrorClassifier';
import { ssoKeytarAccount } from '../../src/main/services/ssoOrgKey';
import { parseSamlRoles } from '../../src/main/services/samlRoles';
import { parseCredentials, removeSection, renameSection, setSection } from '../../src/main/services/credentialsIni';
import type { OrgConfig } from '../../src/shared/orgConfig';
import type { Settings } from '../../src/shared/types';

const DIR = path.join(path.dirname(fileURLToPath(import.meta.url)), '..', '..', 'native', 'tests', 'fixtures', 'vectors');
const RECORD = process.env.VECTORS_RECORD === '1';

/** Build an error-like value from a JSON description (classifier inputs). */
function errorFrom(spec: unknown): unknown {
  if (!spec || typeof spec !== 'object') return spec;
  const s = spec as { error?: string; cause?: unknown; [k: string]: unknown };
  const base: Record<string, unknown> = s.error !== undefined ? Object.assign(new Error(s.error), {}) : {};
  for (const [k, v] of Object.entries(s)) {
    if (k === 'error') continue;
    base[k] = k === 'cause' ? errorFrom(v) : v;
  }
  return base;
}

/** The 1.x master-password envelope with explicit salt and IV (credentialStorage.ts encryptPayload). */
function encryptV1With(password: string, saltHex: string, ivHex: string, plaintext: string): string {
  const salt = Buffer.from(saltHex, 'hex');
  const iv = Buffer.from(ivHex, 'hex');
  const key = crypto.pbkdf2Sync(password, salt, 100_000, 32, 'sha256');
  const c = crypto.createCipheriv('aes-256-gcm', key, iv);
  const enc = Buffer.concat([c.update(plaintext, 'utf8'), c.final()]);
  return 'v1:' + Buffer.concat([salt, iv, enc, c.getAuthTag()]).toString('base64');
}

function decryptV1(password: string, blob: string): string | null {
  try {
    const raw = Buffer.from(blob.slice(3), 'base64');
    if (!blob.startsWith('v1:') || raw.length < 44) return null;
    const key = crypto.pbkdf2Sync(password, raw.subarray(0, 16), 100_000, 32, 'sha256');
    const d = crypto.createDecipheriv('aes-256-gcm', key, raw.subarray(16, 28));
    d.setAuthTag(raw.subarray(raw.length - 16));
    return d.update(raw.subarray(28, raw.length - 16)) + d.final('utf8');
  } catch {
    return null;
  }
}

const FUNCTIONS: Record<string, (...args: never[]) => unknown> = {
  validateMasterPassword: (pw: string) => validateMasterPassword(pw),
  parseOrgConfig: (raw: unknown) => parseOrgConfig(raw),
  describeOrgConfig: (c: OrgConfig) => describeOrgConfig(c),
  isOrgConfigEmpty: (c: OrgConfig) => isOrgConfigEmpty(c),
  applyOrgConfigToSettings: (s: Settings, c: OrgConfig, nowIso: string) => applyOrgConfigToSettings(s, c, new Date(nowIso)),
  groupProfilesByFolder: (p: never[], f: never[]) => groupProfilesByFolder(p, f),
  flattenGrouping: (p: never[], f: never[]) => flattenGrouping(groupProfilesByFolder(p, f)),
  resolveAuthType: (p: never) => resolveAuthType(p),
  normalizeStartUrl: (u: string) => normalizeStartUrl(u),
  orgKey: (o: never) => orgKey(o),
  orgOfProfile: (p: never) => orgOfProfile(p),
  sameOrg: (a: never, b: never) => sameOrg(a, b),
  distinctOrgs: (p: never[]) => distinctOrgs(p),
  resolveTerminalProfileId: (id: string | null, p: never[]) => resolveTerminalProfileId(id ?? undefined, p),
  ssoKeytarAccount: (o: never) => ssoKeytarAccount(o),
  classifyAuthFailure: (e: unknown) => classifyAuthFailure(errorFrom(e)),
  parseSamlRoles: (xml: string) => parseSamlRoles(Buffer.from(xml, 'utf8').toString('base64')),
  credentialsParse: (text: string) => parseCredentials(text),
  credentialsSetSection: (text: string, name: string, values: Record<string, string>, eol: string) => setSection(text, name, values, eol),
  credentialsRenameSection: (text: string, from: string, to: string, eol: string) => renameSection(text, from, to, eol),
  credentialsRemoveSection: (text: string, name: string, eol: string) => removeSection(text, name, eol),
  encryptV1With: (pw: string, salt: string, iv: string, text: string) => encryptV1With(pw, salt, iv, text),
  decryptV1: (pw: string, blob: string) => decryptV1(pw, blob),
};

/** JSON form: undefined keys dropped, undefined -> null (what the Rust side produces). */
const asJson = (v: unknown) => (v === undefined ? null : JSON.parse(JSON.stringify(v)));

/** "$fill:other.json#case name" = the recorded expectation of that case (chains vectors together). */
function fillRef(arg: unknown): unknown {
  if (typeof arg !== 'string' || !arg.startsWith('$fill:')) return arg;
  const [file, name] = arg.slice('$fill:'.length).split('#');
  const other = JSON.parse(fs.readFileSync(path.join(DIR, file), 'utf8')) as VectorFile;
  const hit = other.cases.find((c) => c.name === name);
  if (!hit || hit.expect === '$record') throw new Error(`${arg}: no recorded expectation`);
  return hit.expect;
}

interface VectorFile {
  function: string;
  note?: string;
  cases: Array<{ name: string; args: unknown[]; expect: unknown }>;
}

for (const file of fs.readdirSync(DIR).filter((f) => f.endsWith('.json'))) {
  const full = path.join(DIR, file);
  const data = JSON.parse(fs.readFileSync(full, 'utf8')) as VectorFile;
  const fn = FUNCTIONS[data.function];
  describe(`${file} (${data.function})`, () => {
    it('names a known function', () => expect(fn, `unknown function ${data.function}`).toBeTypeOf('function'));
    for (const c of data.cases) {
      it(c.name, async () => {
        // `null` args stand for "absent" (undefined) where the TS signature allows it.
        const args = c.args.map(fillRef);
        const actual = asJson(await (fn as (...a: unknown[]) => unknown)(...args));
        if (c.expect === '$record') {
          if (!RECORD) throw new Error('expectation not recorded yet (run with VECTORS_RECORD=1)');
          c.expect = actual;
          return;
        }
        expect(actual).toEqual(c.expect);
      });
    }
  });
  if (RECORD) {
    // Write recorded expectations back once every case in the file has run.
    it(`${file}: save recordings`, () => fs.writeFileSync(full, JSON.stringify(data, null, 2) + '\n'));
  }
}
