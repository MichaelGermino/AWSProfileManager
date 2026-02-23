#!/usr/bin/env node
/**
 * Electron main: fetches AWS CLI docs using the same method as the app
 * (hidden BrowserWindow = Chromium network stack / system certs), then parses.
 * Run: npm install && npm start
 * Use on the machine where scraped results don't load to verify the browser path works.
 */

const { app, BrowserWindow } = require('electron');
const path = require('path');

const INDEX_URL = 'https://docs.aws.amazon.com/cli/latest/';
const BASE_REFERENCE = 'https://docs.aws.amazon.com/cli/latest/reference';
const TIMEOUT_MS = 20000;

function fetchHtmlWithBrowser(url) {
  return new Promise((resolve, reject) => {
    const win = new BrowserWindow({
      show: false,
      webPreferences: {
        nodeIntegration: false,
        contextIsolation: true,
      },
    });

    let settled = false;
    function finish(err, html) {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      if (!win.isDestroyed()) win.destroy();
      if (err) reject(err);
      else if (html != null) resolve(html);
    }

    const timer = setTimeout(() => {
      finish(new Error(`Timeout loading ${url}`));
    }, TIMEOUT_MS);

    win.webContents.on('did-fail-load', (_e, errorCode, errorDescription, validatedUrl) => {
      if (validatedUrl === url) {
        finish(new Error(`Load failed: ${errorDescription} (${errorCode})`));
      }
    });

    win.loadURL(url, {
      userAgent:
        'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36',
    }).catch((err) => finish(err));

    win.webContents.once('did-finish-load', () => {
      if (settled) return;
      win.webContents
        .executeJavaScript('document.documentElement.outerHTML')
        .then((html) => finish(null, html))
        .catch((err) => finish(err));
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

async function run() {
  console.log('Fetch method: hidden BrowserWindow (Chromium / system certs, same as app)\n');
  console.log('INDEX_URL:', INDEX_URL);
  console.log('');

  try {
    console.log('Step 1: Fetch index page (BrowserWindow)');
    const indexHtml = await fetchHtmlWithBrowser(INDEX_URL);
    console.log('  Body length:', indexHtml.length, 'bytes');
    console.log('  OK\n');

    console.log('Step 2: Parse service list (li.toctree-l2)');
    const slugs = parseServiceList(indexHtml);
    console.log('  Found', slugs.length, 'services');
    if (slugs.length > 0) {
      console.log('  First 10:', slugs.slice(0, 10).join(', '));
    }
    console.log('');

    const testSlug = 's3';
    const serviceUrl = `${BASE_REFERENCE}/${testSlug}/`;
    console.log('Step 3: Fetch one service page (BrowserWindow):', serviceUrl);
    const serviceHtml = await fetchHtmlWithBrowser(serviceUrl);
    console.log('  Body length:', serviceHtml.length, 'bytes');
    console.log('  OK\n');

    console.log('Step 4: Parse commands (li.toctree-l1) for', testSlug);
    const commands = parseCommands(serviceHtml);
    console.log('  Found', commands.length, 'commands');
    if (commands.length > 0) {
      console.log('  First 8:', commands.slice(0, 8).map((c) => c.name).join(', '));
    }
    console.log('');

    console.log('Done. All steps succeeded (browser method).');
  } catch (err) {
    console.error('\nError:', err.message);
    if (err.stack) console.error(err.stack);
    app.exit(1);
    return;
  }

  app.exit(0);
}

app.whenReady().then(run).catch((err) => {
  console.error('App error:', err);
  app.exit(1);
});

// No window except our hidden one; quit when run() calls app.exit()
app.on('window-all-closed', () => {});
