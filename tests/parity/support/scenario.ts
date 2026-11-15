/**
 * Shared scenario format (docs/native-rewrite-plan.md §5.2) and the matcher both runners use.
 * The Rust runner (native/crates/apm-core/tests/scenarios.rs) implements the same rules; the
 * format reference is native/tests/fixtures/scenarios/README.md.
 */
import crypto from 'node:crypto';

export interface Scenario {
  id: string;
  title: string;
  /** Platforms this scenario applies to; default all. */
  platforms?: Array<'win32' | 'darwin'>;
  given?: {
    dataDir?: string;
    files?: Record<string, unknown>;
    credentials?: string;
    /** Files relative to the sandbox root, e.g. a backup a restore dialog will pick. */
    sandboxFiles?: Record<string, unknown>;
    /** Keychain entries before the app starts: account -> string | {"$encrypt": {...}}. */
    keychain?: Record<string, unknown>;
    /** Start the fake services with this config (tests/fakes/fakes.ts); enables {FAKES}. */
    fakes?: Record<string, unknown>;
    /** Test-hook environment, e.g. {"APM_SCHEDULER_TICK_MS": "300"}. */
    env?: Record<string, string>;
  };
  steps: Step[];
  then?: {
    /** App-data files: { json: matcher } | { text: exact } | { absent: true } */
    files?: Record<string, FileExpectation>;
    /** Files relative to the sandbox root (e.g. a backup the app saved via a dialog). */
    sandboxFiles?: Record<string, FileExpectation>;
    /** The ~/.aws/credentials file. */
    credentials?: FileExpectation;
    /** Keychain entries for the test service: account -> matcher, or "$absent". */
    keychain?: Record<string, unknown>;
    /** Requests the fakes received, per service, in order: [{service, expect: [matcher, ...]}]. */
    fakeRequests?: Array<{ service: string; expect: unknown }>;
  };
}

export type Step =
  | {
      call: string;
      args?: unknown[];
      expect?: unknown;
      /** The call must fail; its message (without Electron's "Error invoking remote method" prefix) matches. */
      expectError?: unknown;
      save?: string;
      /** Start the call without waiting; finish it later with {"await": name}. */
      async?: string;
      note?: string;
    }
  | { await: string; expect?: unknown; save?: string; note?: string }
  /** Items received on a streaming channel since start (e.g. "ai:chat-chunk", "terminal:data"). */
  | { expectStream: string; expect: unknown; note?: string }
  // ---- UI steps (flows). L = locator: {role, name, within?, nth?}; see flows/README.md.
  | { click: Locator; button?: 'left' | 'right'; note?: string }
  | { hover: Locator; note?: string }
  | { focus: Locator; note?: string }
  /** Press and hold (keyboard Space on the focused element) for ms milliseconds. */
  | { hold: Locator; ms?: number; note?: string }
  | { fill: Locator; value: string; note?: string }
  | { select: Locator; option: string; note?: string }
  | { press: string; note?: string }
  | { type: string; note?: string }
  | { see: Locator; state?: UiState; count?: number; text?: string; value?: string; note?: string }
  /** Forget what the fakes have recorded so far (then.fakeRequests checks only what follows). */
  | { resetFakeRequests: true; note?: string }
  /** Move the backend's clock (Date.now) forward by this many milliseconds. */
  | { advanceClock: number; note?: string }
  /** Real-time pause (e.g. let a fast scheduler tick run). */
  | { wait: number; note?: string }
  /** URLs/paths the app tried to open externally since the previous check: [{kind, target}]. */
  | { expectExternalOpens: unknown; waitMs?: number; note?: string }
  /** Wait until a streaming channel has at least `count` items, or its joined text contains `text`. */
  | { waitForStream: string; count?: number; text?: string; timeoutMs?: number; note?: string }
  | { restart: true; note?: string }
  /** Events received since the previous expectEvents (or launch), in order: [channel, ...payload]. */
  | { expectEvents: unknown[][]; note?: string }
  /** Answers for the next native file dialogs, in order: a path (may use {SANDBOX}) or null = cancel. */
  | { dialogs: Array<string | null>; note?: string };

/** An element found by ARIA role and accessible name (Playwright getByRole / UI Automation). */
export interface Locator {
  role: string;
  /** Exact accessible name, or "$match:<regex>". Omit to match any name. */
  name?: string;
  /** Search inside this element instead of the whole window. */
  within?: Locator;
  /** Pick the n-th match (0-based) when several elements match. */
  nth?: number;
  /** Which window: the main window (default) or another one the app opened. */
  window?: 'main' | 'logViewer';
}

export type UiState = 'visible' | 'hidden' | 'enabled' | 'disabled' | 'checked' | 'unchecked' | 'selected' | 'expanded' | 'collapsed' | 'focused';

/** The 1.x master-password envelope: "v1:" + base64(salt16 | iv12 | AES-256-GCM ciphertext | tag16). */
export function encryptV1(password: string, plaintext: string): string {
  const salt = crypto.randomBytes(16);
  const iv = crypto.randomBytes(12);
  const key = crypto.pbkdf2Sync(password, salt, 100_000, 32, 'sha256');
  const c = crypto.createCipheriv('aes-256-gcm', key, iv);
  const enc = Buffer.concat([c.update(plaintext, 'utf8'), c.final()]);
  return 'v1:' + Buffer.concat([salt, iv, enc, c.getAuthTag()]).toString('base64');
}

export function decryptV1(password: string, blob: string): string | null {
  try {
    const raw = Buffer.from(blob.slice(3), 'base64');
    const key = crypto.pbkdf2Sync(password, raw.subarray(0, 16), 100_000, 32, 'sha256');
    const d = crypto.createDecipheriv('aes-256-gcm', key, raw.subarray(16, 28));
    d.setAuthTag(raw.subarray(raw.length - 16));
    return d.update(raw.subarray(28, raw.length - 16)) + d.final('utf8');
  } catch {
    return null;
  }
}

/** given.keychain value: a plain string, or {"$encrypt": {"password", "plaintext" | "json"}}. */
export function materializeSecret(v: unknown): string {
  if (typeof v === 'string') return v;
  const e = (v as { $encrypt?: { password: string; plaintext?: string; json?: unknown } }).$encrypt;
  if (!e) throw new Error(`bad keychain seed ${JSON.stringify(v)}`);
  return encryptV1(e.password, e.plaintext ?? JSON.stringify(e.json));
}

/** Replace {NAME} tokens (e.g. {SANDBOX}, {FAKES}) in every string, keys included. */
export function withTokens(value: unknown, tokens: Record<string, string>): unknown {
  const sub = (s: string) => Object.entries(tokens).reduce((acc, [k, v]) => acc.replaceAll(`{${k}}`, v), s);
  if (typeof value === 'string') return sub(value);
  if (Array.isArray(value)) return value.map((v) => withTokens(v, tokens));
  if (value && typeof value === 'object') {
    return Object.fromEntries(Object.entries(value).map(([k, v]) => [sub(k), withTokens(v, tokens)]));
  }
  return value;
}

export type FileExpectation =
  | { json: unknown }
  | { text: string }
  | { contains?: string[]; notContains?: string[] }
  | { absent: true };

export type Refs = Record<string, unknown>;

const PLACEHOLDERS: Record<string, (v: unknown) => boolean> = {
  $any: () => true,
  $string: (v) => typeof v === 'string',
  $nonEmptyString: (v) => typeof v === 'string' && v.length > 0,
  $number: (v) => typeof v === 'number',
  $boolean: (v) => typeof v === 'boolean',
  $uuid: (v) => typeof v === 'string' && /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i.test(v),
  $iso: (v) => typeof v === 'string' && !Number.isNaN(Date.parse(v)) && /\d{4}-\d{2}-\d{2}T/.test(v),
  $v1cipher: (v) => typeof v === 'string' && v.startsWith('v1:'),
};

function getPath(obj: unknown, path: string[]): unknown {
  return path.reduce<unknown>((o, k) => (o == null ? undefined : (o as Record<string, unknown>)[k]), obj);
}

/** Replace {"$ref": "name.path"} with saved values (used in args and expectations). */
export function resolveRefs(value: unknown, refs: Refs): unknown {
  if (Array.isArray(value)) return value.map((v) => resolveRefs(v, refs));
  if (value && typeof value === 'object') {
    const o = value as Record<string, unknown>;
    if (typeof o.$ref === 'string' && Object.keys(o).length === 1) {
      const [name, ...rest] = o.$ref.split('.');
      return getPath(refs[name], rest);
    }
    return Object.fromEntries(Object.entries(o).map(([k, v]) => [k, resolveRefs(v, refs)]));
  }
  return value;
}

/**
 * Returns a list of mismatches (empty = match). Rules:
 * - strings starting with "$" listed in PLACEHOLDERS match by predicate;
 * - objects match exactly (same keys) unless the expectation has "$partial": true;
 * - an expected value of "$absent" means the key must not exist;
 * - arrays match element-wise with equal length;
 * - everything else uses strict equality.
 */
export function match(actual: unknown, expected: unknown, at = '$'): string[] {
  // {"$decrypt": {"password": "...", "equals": <matcher>}}: a v1: blob whose plaintext matches;
  // "json" instead of "equals" parses the plaintext as JSON first.
  if (expected && typeof expected === 'object' && '$decrypt' in (expected as object)) {
    const d = (expected as { $decrypt: { password: string; equals?: unknown; json?: unknown } }).$decrypt;
    if (typeof actual !== 'string' || !actual.startsWith('v1:')) return [`${at}: expected v1: ciphertext, got ${JSON.stringify(actual)}`];
    const plain = decryptV1(d.password, actual);
    if (plain === null) return [`${at}: does not decrypt with the given password`];
    if ('json' in d) {
      try {
        return match(JSON.parse(plain), d.json, `${at}<decrypted json>`);
      } catch {
        return [`${at}: decrypted text is not JSON`];
      }
    }
    return match(plain, d.equals, `${at}<decrypted>`);
  }
  // {"$each": m, "$minItems": n}: an array of at least n items (default 1), every one matching m.
  if (expected && typeof expected === 'object' && '$each' in (expected as object)) {
    const e = expected as { $each: unknown; $minItems?: number };
    if (!Array.isArray(actual)) return [`${at}: expected array, got ${JSON.stringify(actual)}`];
    const min = e.$minItems ?? 1;
    if (actual.length < min) return [`${at}: expected at least ${min} items, got ${actual.length}`];
    return actual.flatMap((a, i) => match(a, e.$each, `${at}[${i}]`));
  }
  if (typeof expected === 'string' && expected.startsWith('$match:')) {
    const re = new RegExp(expected.slice('$match:'.length));
    return typeof actual === 'string' && re.test(actual) ? [] : [`${at}: expected to match /${re.source}/, got ${JSON.stringify(actual)}`];
  }
  if (typeof expected === 'string' && expected in PLACEHOLDERS) {
    return PLACEHOLDERS[expected](actual) ? [] : [`${at}: expected ${expected}, got ${JSON.stringify(actual)}`];
  }
  if (Array.isArray(expected)) {
    if (!Array.isArray(actual)) return [`${at}: expected array, got ${JSON.stringify(actual)}`];
    if (actual.length !== expected.length) {
      return [`${at}: expected ${expected.length} items, got ${actual.length}: ${JSON.stringify(actual)}`];
    }
    return expected.flatMap((e, i) => match(actual[i], e, `${at}[${i}]`));
  }
  if (expected && typeof expected === 'object') {
    if (!actual || typeof actual !== 'object' || Array.isArray(actual)) {
      return [`${at}: expected object, got ${JSON.stringify(actual)}`];
    }
    const e = expected as Record<string, unknown>;
    const a = actual as Record<string, unknown>;
    const partial = e.$partial === true;
    const out: string[] = [];
    for (const [k, v] of Object.entries(e)) {
      if (k === '$partial') continue;
      if (v === '$absent') {
        if (k in a) out.push(`${at}.${k}: expected absent, got ${JSON.stringify(a[k])}`);
        continue;
      }
      if (v === '$optional') continue; // may be present with any value, or absent
      if (typeof v === 'string' && v.startsWith('$optional:')) {
        if (k in a) out.push(...match(a[k], '$' + v.slice('$optional:'.length), `${at}.${k}`));
        continue;
      }
      if (!(k in a)) {
        out.push(`${at}.${k}: missing`);
        continue;
      }
      out.push(...match(a[k], v, `${at}.${k}`));
    }
    if (!partial) {
      for (const k of Object.keys(a)) if (!(k in e)) out.push(`${at}.${k}: unexpected key (value ${JSON.stringify(a[k])})`);
    }
    return out;
  }
  return Object.is(actual, expected) ? [] : [`${at}: expected ${JSON.stringify(expected)}, got ${JSON.stringify(actual)}`];
}
