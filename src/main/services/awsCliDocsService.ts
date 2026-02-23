/**
 * Scrapes AWS CLI reference docs for service list and per-service commands.
 * Caches results on disk to avoid repeated requests.
 * @see https://docs.aws.amazon.com/cli/latest/
 */

import https from 'https';
import fs from 'fs';
import path from 'path';
import { load } from 'cheerio';
import { app } from 'electron';

const INDEX_URL = 'https://docs.aws.amazon.com/cli/latest/';
const BASE_REFERENCE = 'https://docs.aws.amazon.com/cli/latest/reference';
const CACHE_TTL_MS = 24 * 60 * 60 * 1000; // 24 hours

function getCacheDir(): string {
  const dir = path.join(app.getPath('userData'), 'aws-cli-docs-cache');
  if (!fs.existsSync(dir)) fs.mkdirSync(dir, { recursive: true });
  return dir;
}

function fetchHtml(url: string): Promise<string> {
  return new Promise((resolve, reject) => {
    const opts: https.RequestOptions = {
      headers: {
        Accept: 'text/html,application/xhtml+xml',
        'User-Agent':
          'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36',
      },
    };
    https
      .get(url, opts, (res) => {
        if (res.statusCode === 301 || res.statusCode === 302) {
          const loc = res.headers.location;
          if (loc) {
            const next = loc.startsWith('http') ? loc : new URL(loc, url).href;
            return fetchHtml(next).then(resolve).catch(reject);
          }
        }
        if (res.statusCode !== 200) {
          reject(new Error(`${url} returned ${res.statusCode}`));
          return;
        }
        const chunks: Buffer[] = [];
        res.on('data', (chunk: Buffer) => chunks.push(chunk));
        res.on('end', () => resolve(Buffer.concat(chunks).toString('utf8')));
      })
      .on('error', reject);
  });
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
 * Returns cached list of service slugs, or fetches index and caches.
 */
export async function getServiceList(): Promise<string[]> {
  const cacheDir = getCacheDir();
  const cachePath = path.join(cacheDir, 'services.json');

  const read = (): { slugs: string[]; at: number } | null => {
    try {
      const raw = fs.readFileSync(cachePath, 'utf8');
      const data = JSON.parse(raw) as { slugs: string[]; at: number };
      if (data.slugs && Array.isArray(data.slugs) && data.at) return data;
    } catch {
      // ignore
    }
    return null;
  };

  const cached = read();
  if (cached && Date.now() - cached.at < CACHE_TTL_MS) {
    return cached.slugs;
  }

  const html = await fetchHtml(INDEX_URL);
  const slugs = parseServiceList(html);
  fs.writeFileSync(cachePath, JSON.stringify({ slugs, at: Date.now() }), 'utf8');
  return slugs;
}

/**
 * Returns commands for a service (cached or scraped). Each command has docUrl.
 */
export async function getCommandsForService(serviceSlug: string): Promise<ScrapedCommand[]> {
  const slug = serviceSlug.replace(/\/$/, '').replace(/\.html$/, '').trim();
  const cacheDir = getCacheDir();
  const cachePath = path.join(cacheDir, `${slug}.json`);

  const read = (): { commands: ScrapedCommand[]; at: number } | null => {
    try {
      const raw = fs.readFileSync(cachePath, 'utf8');
      const data = JSON.parse(raw) as { commands: ScrapedCommand[]; at: number };
      if (data.commands && Array.isArray(data.commands) && data.at) return data;
    } catch {
      // ignore
    }
    return null;
  };

  const cached = read();
  if (cached && Date.now() - cached.at < CACHE_TTL_MS) {
    return cached.commands;
  }

  const url = `${BASE_REFERENCE}/${slug}/`;
  const html = await fetchHtml(url);
  const entries = parseCommands(html, slug);
  const baseUrl = `${BASE_REFERENCE}/${slug}`;
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

  fs.writeFileSync(cachePath, JSON.stringify({ commands, at: Date.now() }), 'utf8');
  return commands;
}
