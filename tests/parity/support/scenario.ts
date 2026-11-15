/**
 * Shared scenario format (docs/native-rewrite-plan.md §5.2) and the matcher both runners use.
 * The Rust runner (native/crates/apm-core/tests/scenarios.rs) implements the same rules; the
 * format reference is native/tests/fixtures/scenarios/README.md.
 */

export interface Scenario {
  id: string;
  title: string;
  /** Platforms this scenario applies to; default all. */
  platforms?: Array<'win32' | 'darwin'>;
  given?: {
    dataDir?: string;
    files?: Record<string, unknown>;
    credentials?: string;
  };
  steps: Step[];
  then?: {
    /** App-data files: { json: matcher } | { text: exact } | { absent: true } */
    files?: Record<string, FileExpectation>;
    /** The ~/.aws/credentials file. */
    credentials?: FileExpectation;
    /** Keychain entries for the test service: account -> matcher, or "$absent". */
    keychain?: Record<string, unknown>;
  };
}

export type Step =
  | { call: string; args?: unknown[]; expect?: unknown; save?: string; note?: string }
  | { restart: true; note?: string };

export type FileExpectation = { json: unknown } | { text: string } | { contains: string[] } | { absent: true };

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
