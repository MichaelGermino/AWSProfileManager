/**
 * Minimal, lossless editor for ~/.aws/credentials. Pure (no fs, no Electron) so the parity suite's
 * vectors test it directly (native/tests/fixtures/vectors/credentials-ini-*.json) and the Rust port
 * can match it byte for byte.
 *
 * Rules:
 * - Only the section being written, renamed or removed changes. Every other byte of the file —
 *   other tools' sections, comments, spacing, key order — is kept exactly.
 * - The file's own line-ending style is kept (CRLF if it has any CRLF, else LF); a new file uses
 *   the platform default passed in by the caller.
 * - Section names are taken literally ("dev.admin" is one section, not nesting).
 * - Values are split at the first "=" only (session tokens end in "=").
 *
 * Replaces the `ini` package, which re-serialised the whole file on every write (dropping
 * comments and reformatting other tools' sections) and treated dots in section names as nesting.
 */

export type Section = Record<string, string>;

const HEADER = /^\s*\[([^\]]*)\]\s*$/;
const COMMENT = /^\s*[#;]/;

export function detectEol(text: string, fallback: string): string {
  if (text.includes('\r\n')) return '\r\n';
  if (text.includes('\n')) return '\n';
  return fallback;
}

function splitLines(text: string): string[] {
  if (text === '') return [];
  const lines = text.split(/\r?\n/);
  // A trailing newline yields one empty last element; drop it (re-added on join).
  if (lines[lines.length - 1] === '') lines.pop();
  return lines;
}

/** Sections by name. Later duplicates of a key win; later duplicate sections merge. */
export function parseCredentials(text: string): Record<string, Section> {
  const out: Record<string, Section> = {};
  let current: Section | null = null;
  for (const line of splitLines(text)) {
    const header = HEADER.exec(line);
    if (header) {
      const name = header[1].trim();
      current = out[name] ?? (out[name] = {});
      continue;
    }
    if (!current || COMMENT.test(line) || !line.includes('=')) continue;
    const eq = line.indexOf('=');
    const key = line.slice(0, eq).trim();
    if (key) current[key] = line.slice(eq + 1).trim();
  }
  return out;
}

/**
 * A section: comment lines directly above its header (no blank line between) belong to it, then
 * the header, its keys, and whatever blank/comment lines follow until the next section's block.
 */
interface Block {
  name: string | null; // null = lines before the first section
  lines: string[];
  headerAt: number; // index of the header line in `lines` (-1 for the leading block)
}

function blocks(lines: string[]): Block[] {
  const out: Block[] = [{ name: null, lines: [], headerAt: -1 }];
  for (const line of lines) {
    const header = HEADER.exec(line);
    if (!header) {
      out[out.length - 1].lines.push(line);
      continue;
    }
    // Move the comment lines immediately above this header from the previous block into this one.
    const prev = out[out.length - 1];
    let k = prev.lines.length;
    while (k > prev.headerAt + 1 && COMMENT.test(prev.lines[k - 1])) k--;
    const leading = prev.lines.splice(k);
    out.push({ name: header[1].trim(), lines: [...leading, line], headerAt: leading.length });
  }
  return out;
}

/** Index just past the block's last key line: blank lines and comments after it are its "tail". */
function contentEnd(b: Block): number {
  let end = b.lines.length;
  while (end > b.headerAt + 1 && (b.lines[end - 1].trim() === '' || COMMENT.test(b.lines[end - 1]))) end--;
  return end;
}

/** After removing sections: no blank lines left dangling at the start or end of the file. */
function tidyEdges(lines: string[]): string[] {
  let start = 0;
  let end = lines.length;
  while (start < end && lines[start].trim() === '') start++;
  while (end > start && lines[end - 1].trim() === '') end--;
  return lines.slice(start, end);
}

function join(lines: string[], eol: string): string {
  return lines.length ? lines.join(eol) + eol : '';
}

/**
 * Write (replace or append) one section. Keys are written as `key=value` in the given order.
 * `defaultEol` is used only when the file has no line breaks yet.
 */
export function setSection(text: string, name: string, values: Section, defaultEol: string): string {
  const eol = detectEol(text, defaultEol);
  const body = Object.entries(values).map(([k, v]) => `${k}=${v}`);
  const all = blocks(splitLines(text));
  const target = all.find((b) => b.name === name);
  if (target) {
    const head = target.lines.slice(0, target.headerAt + 1);
    const tail = target.lines.slice(contentEnd(target));
    target.lines = [...head, ...body, ...tail];
    // Later sections with the same name would shadow this one in other tools; drop them.
    const rest = all.filter((b) => b === target || b.name !== name);
    const lines = rest.flatMap((b) => b.lines);
    return join(rest.length === all.length ? lines : tidyEdges(lines), eol);
  }
  const lines = all.flatMap((b) => b.lines);
  if (lines.length && lines[lines.length - 1].trim() !== '') lines.push('');
  lines.push(`[${name}]`, ...body);
  return join(lines, eol);
}

/** Rename a section's header in place; its keys, comments and position stay. False if absent. */
export function renameSection(text: string, from: string, to: string, defaultEol: string): string | null {
  const eol = detectEol(text, defaultEol);
  const all = blocks(splitLines(text));
  const target = all.find((b) => b.name === from);
  if (!target || from === to) return null;
  target.lines[target.headerAt] = target.lines[target.headerAt].replace(/\[([^\]]*)\]/, `[${to}]`);
  target.name = to;
  return join(all.flatMap((b) => b.lines), eol);
}

/**
 * Remove a section: the comments directly above it, its header, keys and trailing lines up to the
 * next section. Null if absent.
 */
export function removeSection(text: string, name: string, defaultEol: string): string | null {
  const eol = detectEol(text, defaultEol);
  const all = blocks(splitLines(text));
  if (!all.some((b) => b.name === name)) return null;
  const kept = all.filter((b) => b.name !== name);
  return join(tidyEdges(kept.flatMap((b) => b.lines)), eol);
}
