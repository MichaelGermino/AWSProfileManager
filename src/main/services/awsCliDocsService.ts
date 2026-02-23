/**
 * AWS CLI reference docs: parses HTML and caches on disk.
 * Fetch is done in main via hidden BrowserWindow (Chromium / system certs).
 * @see https://docs.aws.amazon.com/cli/latest/
 */

import fs from 'fs';
import path from 'path';
import { load } from 'cheerio';
import { app } from 'electron';

export const AWS_CLI_INDEX_URL = 'https://docs.aws.amazon.com/cli/latest/';
export const AWS_CLI_BASE_REFERENCE = 'https://docs.aws.amazon.com/cli/latest/reference';
const CACHE_TTL_MS = 24 * 60 * 60 * 1000; // 24 hours

function getCacheDir(): string {
  const dir = path.join(app.getPath('userData'), 'aws-cli-docs-cache');
  if (!fs.existsSync(dir)) fs.mkdirSync(dir, { recursive: true });
  return dir;
}

/**
 * Parse index page for service slugs from li.toctree-l2 > a.reference.internal.
 * Example: <li class="toctree-l2"><a class="reference internal" href="reference/partnercentral-channel/index.html">partnercentral-channel</a></li>
 */
function parseServiceList(html: string): string[] {
  const $ = load(html);
  const slugs: string[] = [];
  const seen = new Set<string>();
  $('li.toctree-l2 a.reference.internal[href*="reference/"]').each((_, el) => {
    const href = $(el).attr('href') ?? '';
    const match = href.match(/reference\/([^/]+)(?:\/|$)/);
    if (match) {
      const slug = match[1].replace(/\.html$/, '').trim();
      if (slug && !seen.has(slug)) {
        seen.add(slug);
        slugs.push(slug);
      }
    }
  });
  return slugs.sort((a, b) => a.localeCompare(b));
}

/**
 * Parse service reference page for commands from li.toctree-l1 > a.reference.internal.
 * Example: <li class="toctree-l1"><a class="reference internal" href="mb.html">mb</a></li>
 */
function parseCommands(html: string, serviceSlug: string): { name: string; href: string }[] {
  const $ = load(html);
  const commands: { name: string; href: string }[] = [];
  const validName = /^[a-z0-9][a-z0-9-]*$/;
  $('li.toctree-l1 a.reference.internal[href]').each((_, el) => {
    const $a = $(el);
    const href = ($a.attr('href') ?? '').trim();
    const text = $a.text().trim();
    const name = text || path.basename(href, '.html');
    if (!name || !validName.test(name) || commands.some((c) => c.name === name)) return;
    commands.push({ name, href });
  });
  return commands;
}

export interface ScrapedCommand {
  id: string;
  name: string;
  description: string;
  syntax: string;
  options: unknown[];
  examples: unknown[];
  mocked: false;
  docUrl: string;
}

/**
 * Returns cached service list if valid; otherwise null.
 */
export function getCachedServiceList(): string[] | null {
  const cacheDir = getCacheDir();
  const cachePath = path.join(cacheDir, 'services.json');
  try {
    const raw = fs.readFileSync(cachePath, 'utf8');
    const data = JSON.parse(raw) as { slugs: string[]; at: number };
    if (data.slugs && Array.isArray(data.slugs) && data.at && Date.now() - data.at < CACHE_TTL_MS) {
      return data.slugs;
    }
  } catch {
    // ignore
  }
  return null;
}

/**
 * Parse index HTML and cache; returns service slugs. Call after fetch.
 */
export function parseAndCacheServiceList(html: string): string[] {
  const slugs = parseServiceList(html);
  const cacheDir = getCacheDir();
  const cachePath = path.join(cacheDir, 'services.json');
  fs.writeFileSync(cachePath, JSON.stringify({ slugs, at: Date.now() }), 'utf8');
  return slugs;
}

/**
 * Returns cached commands for a service if valid; otherwise null (caller fetches HTML and calls parseAndCacheCommands).
 */
export function getCachedCommands(serviceSlug: string): ScrapedCommand[] | null {
  const slug = serviceSlug.replace(/\/$/, '').replace(/\.html$/, '').trim();
  const cacheDir = getCacheDir();
  const cachePath = path.join(cacheDir, `${slug}.json`);
  try {
    const raw = fs.readFileSync(cachePath, 'utf8');
    const data = JSON.parse(raw) as { commands: ScrapedCommand[]; at: number };
    if (
      data.commands &&
      Array.isArray(data.commands) &&
      data.at &&
      Date.now() - data.at < CACHE_TTL_MS
    ) {
      return data.commands;
    }
  } catch {
    // ignore
  }
  return null;
}

/**
 * Parse service page HTML and cache; returns commands. Call after renderer fetches service URL.
 */
export function parseAndCacheCommands(serviceSlug: string, html: string): ScrapedCommand[] {
  const slug = serviceSlug.replace(/\/$/, '').replace(/\.html$/, '').trim();
  const entries = parseCommands(html, slug);
  const baseUrl = `${AWS_CLI_BASE_REFERENCE}/${slug}`;
  const commands: ScrapedCommand[] = entries.map((e) => {
    const docUrl = e.href.startsWith('http') ? e.href : `${baseUrl}/${e.href.replace(/^\//, '')}`;
    return {
      id: `${slug}-${e.name}`,
      name: e.name,
      description: '',
      syntax: `aws ${slug} ${e.name} [options]`,
      options: [],
      examples: [],
      mocked: false,
      docUrl,
    };
  });
  const cacheDir = getCacheDir();
  const cachePath = path.join(cacheDir, `${slug}.json`);
  fs.writeFileSync(cachePath, JSON.stringify({ commands, at: Date.now() }), 'utf8');
  return commands;
}
