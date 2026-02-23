#!/usr/bin/env node
/**
 * Node (https) path for debugging AWS CLI docs fetch/parse.
 * Run: npm run test-node
 *
 * By default use "npm start" instead — that runs the same method as the app
 * (Electron hidden BrowserWindow = Chromium / system certs). This script uses
 * Node's https and can fail with "self-signed certificate" behind corporate
 * TLS inspection. For that case (debug only):
 *   set DEBUG_AWS_CLI_INSECURE_SSL=1   (Windows)
 *   DEBUG_AWS_CLI_INSECURE_SSL=1 npm run test-node   (Unix)
 * Or add your corporate CA: set NODE_EXTRA_CA_CERTS=c:\path\to\ca.pem
 */

const https = require('https');
const path = require('path');

const INDEX_URL = 'https://docs.aws.amazon.com/cli/latest/';
const BASE_REFERENCE = 'https://docs.aws.amazon.com/cli/latest/reference';

const allowInsecure = process.env.DEBUG_AWS_CLI_INSECURE_SSL === '1';
if (allowInsecure) {
  console.log('(DEBUG_AWS_CLI_INSECURE_SSL=1: accepting TLS certificates for debugging)\n');
}

function fetchHtml(url) {
  return new Promise((resolve, reject) => {
    const opts = {
      headers: {
        Accept: 'text/html,application/xhtml+xml',
        'User-Agent':
          'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36',
      },
    };
    if (allowInsecure) {
      opts.rejectUnauthorized = false;
    }
    console.log(`  GET ${url}`);
    https
      .get(url, opts, (res) => {
        console.log(`  Status: ${res.statusCode} ${res.statusMessage}`);
        if (res.headers.location) {
          console.log(`  Redirect: ${res.headers.location}`);
        }
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
        const chunks = [];
        res.on('data', (chunk) => chunks.push(chunk));
        res.on('end', () => {
          const body = Buffer.concat(chunks).toString('utf8');
          console.log(`  Body length: ${body.length} bytes`);
          resolve(body);
        });
      })
      .on('error', (err) => {
        console.error('  Request error:', err.message);
        reject(err);
      });
  });
}

function parseServiceList(html) {
  const { load } = require('cheerio');
  const $ = load(html);
  const slugs = [];
  const seen = new Set();
  $('li.toctree-l2 a.reference.internal[href*="reference/"]').each((_, el) => {
    const href = $(el).attr('href') || '';
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

function parseCommands(html) {
  const { load } = require('cheerio');
  const $ = load(html);
  const commands = [];
  const validName = /^[a-z0-9][a-z0-9-]*$/;
  $('li.toctree-l1 a.reference.internal[href]').each((_, el) => {
    const $a = $(el);
    const href = ($a.attr('href') || '').trim();
    const text = $a.text().trim();
    const name = text || path.basename(href, '.html');
    if (!name || !validName.test(name) || commands.some((c) => c.name === name)) return;
    commands.push({ name, href });
  });
  return commands;
}

async function main() {
  console.log('Node version:', process.version);
  console.log('INDEX_URL:', INDEX_URL);
  console.log('');

  try {
    console.log('Step 1: Fetch index page');
    const indexHtml = await fetchHtml(INDEX_URL);
    console.log('  OK\n');

    console.log('Step 2: Parse service list (li.toctree-l2)');
    const slugs = parseServiceList(indexHtml);
    console.log(`  Found ${slugs.length} services`);
    if (slugs.length > 0) {
      console.log('  First 10:', slugs.slice(0, 10).join(', '));
    }
    console.log('');

    const testSlug = 's3';
    const serviceUrl = `${BASE_REFERENCE}/${testSlug}/`;
    console.log('Step 3: Fetch one service page:', serviceUrl);
    const serviceHtml = await fetchHtml(serviceUrl);
    console.log('  OK\n');

    console.log('Step 4: Parse commands (li.toctree-l1) for', testSlug);
    const commands = parseCommands(serviceHtml);
    console.log(`  Found ${commands.length} commands`);
    if (commands.length > 0) {
      console.log('  First 8:', commands.slice(0, 8).map((c) => c.name).join(', '));
    }
    console.log('');

    console.log('Done. All steps succeeded.');
  } catch (err) {
    console.error('\nError:', err.message);
    if (err.message && err.message.includes('certificate')) {
      console.error(
        '\nThis is usually caused by corporate TLS inspection (proxy). The browser trusts your\n' +
          'corporate CA; Node does not by default. To run this script for debugging only:\n' +
          '  Windows: set DEBUG_AWS_CLI_INSECURE_SSL=1 && npm start\n' +
          '  Unix:    DEBUG_AWS_CLI_INSECURE_SSL=1 npm start\n' +
          'Or add your corporate CA: set NODE_EXTRA_CA_CERTS=c:\\path\\to\\your-ca.pem'
      );
    }
    if (err.stack) {
      console.error(err.stack);
    }
    process.exit(1);
  }
}

main();
