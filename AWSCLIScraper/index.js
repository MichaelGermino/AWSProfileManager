/**
 * AWS CLI reference docs scraper.
 * Discovers services from https://docs.aws.amazon.com/cli/latest/ (li.toctree-l2 links),
 * fetches each service page, extracts available commands from div#available-commands,
 * merges with awsclimockdata.json, and writes to ../src/renderer/terminal/awsclidata.json
 */

const https = require('https');
const fs = require('fs');
const path = require('path');
const { load } = require('cheerio');

const INDEX_URL = 'https://docs.aws.amazon.com/cli/latest/';
const BASE_URL = 'https://docs.aws.amazon.com/cli/latest/reference';
const RATE_LIMIT_MS = 1500;
const TERMINAL_DIR = path.join(__dirname, '..', 'src', 'renderer', 'terminal');
const OUTPUT_PATH = path.join(TERMINAL_DIR, 'awsclidata.json');
const MOCK_DATA_PATH = path.join(TERMINAL_DIR, 'awsclimockdata.json');

function fetchHtml(url) {
  return new Promise((resolve, reject) => {
    const opts = {
      headers: {
        'Accept': 'text/html,application/xhtml+xml',
        'User-Agent': 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36',
      },
    };
    https
      .get(url, opts, (res) => {
        if (res.statusCode === 301 || res.statusCode === 302) {
          const loc = res.headers.location;
          if (loc) return fetchHtml(loc.startsWith('http') ? loc : new URL(loc, url).href).then(resolve).catch(reject);
        }
        if (res.statusCode !== 200) {
          reject(new Error(`${url} returned ${res.statusCode}`));
          return;
        }
        const chunks = [];
        res.on('data', (chunk) => chunks.push(chunk));
        res.on('end', () => resolve(Buffer.concat(chunks).toString('utf8')));
      })
      .on('error', reject);
  });
}

function sleep(ms) {
  return new Promise((r) => setTimeout(r, ms));
}

/**
 * Fetch the CLI index page and extract service slugs from li.toctree-l2 links.
 * @returns {Promise<string[]>} Sorted, unique list of service slugs
 */
async function getServiceListFromIndex() {
  const html = await fetchHtml(INDEX_URL);
  const $ = load(html);
  const slugs = [];
  const seen = new Set();
  // Services are in li.toctree-l2 with a.reference.internal href like "reference/s3/index.html"
  $('li.toctree-l2 a.reference.internal[href*="reference/"]').each((_, el) => {
    const href = $(el).attr('href') || '';
    const match = href.match(/reference\/([^/]+)(?:\/|$)/);
    if (match) {
      const slug = match[1].replace(/\.html$/, '');
      if (slug && !seen.has(slug)) {
        seen.add(slug);
        slugs.push(slug);
      }
    }
  });
  return slugs.sort((a, b) => a.localeCompare(b));
}

/**
 * @param {string} slug - Service slug (e.g. cognito-idp, s3)
 * @returns {Promise<{ slug: string, description: string, commands: { name: string }[] } | null>}
 */
async function scrapeService(slug) {
  const url = `${BASE_URL}/${slug}/`;
  try {
    const html = await fetchHtml(url);
    const $ = load(html);

    let description = '';
    const descSection = $('#description').parent().next();
    if (descSection.length) {
      description = descSection.text().trim().replace(/\s+/g, ' ').slice(0, 500);
    }

    const commands = [];
    const validName = /^[a-z0-9][a-z0-9-]*$/;
    $('#available-commands').find('a[href]').each((_, el) => {
      const $a = $(el);
      const href = $a.attr('href') || '';
      const text = $a.text().trim();
      let name = text || (href.replace(/\.html$/, '').split('/').pop() || '').replace(/\.html$/, '');
      if (!name || !validName.test(name) || commands.some((c) => c.name === name)) return;
      commands.push({ name });
    });

    return { slug, description, commands };
  } catch (err) {
    console.error(`Failed ${slug}: ${err.message}`);
    return null;
  }
}

/**
 * Build one AwsCliCommand for the service root and minimal children.
 * @param {{ slug: string, description: string, commands: { name: string }[] }} data
 * @returns {{ id: string, name: string, description: string, syntax: string, options: [], examples: [], children: object[] }}
 */
function toAwsCliCommand(data) {
  const { slug, description, commands } = data;
  const children = commands.map((c) => ({
    id: `${slug}-${c.name}`,
    name: c.name,
    description: '',
    syntax: `aws ${slug} ${c.name} [options]`,
    options: [],
    examples: [],
    mocked: false,
  }));

  return {
    id: slug,
    name: slug,
    description: description || `AWS CLI service: ${slug}`,
    syntax: `aws ${slug} <command> [options]`,
    options: [],
    examples: [],
    mocked: false,
    children,
  };
}

async function main() {
  const cliSlugs = process.argv.slice(2).filter((a) => !a.startsWith('-'));
  let slugs;
  if (cliSlugs.length > 0) {
    slugs = cliSlugs;
    console.log(`Using ${slugs.length} service(s) from CLI args`);
  } else {
    console.log('Fetching service list from', INDEX_URL);
    slugs = await getServiceListFromIndex();
    console.log(`Found ${slugs.length} service(s) in index`);
  }

  console.log(`Scraping ${slugs.length} service(s)...`);
  const results = [];

  for (let i = 0; i < slugs.length; i++) {
    const slug = slugs[i].replace(/\/$/, '').replace(/\.html$/, '');
    process.stdout.write(`  [${i + 1}/${slugs.length}] ${slug} ... `);
    const data = await scrapeService(slug);
    if (data && data.commands.length > 0) {
      results.push(toAwsCliCommand(data));
      console.log(`${data.commands.length} commands`);
    } else if (data) {
      console.log('no commands found');
    } else {
      console.log('failed');
    }
    if (i < slugs.length - 1) await sleep(RATE_LIMIT_MS);
  }

  // Merge: replace scraped entries with mock data where we have it (by service id)
  let merged = results;
  if (fs.existsSync(MOCK_DATA_PATH)) {
    const mockList = JSON.parse(fs.readFileSync(MOCK_DATA_PATH, 'utf8'));
    const mockById = new Map(mockList.map((s) => [s.id, s]));
    merged = results.map((scraped) => (mockById.has(scraped.id) ? mockById.get(scraped.id) : scraped));
    const replaced = merged.filter((s) => s.mocked === true).length;
    if (replaced > 0) {
      console.log(`\nMerged: ${replaced} service(s) replaced with mock data from awsclimockdata.json`);
    }
  }

  const outDir = path.dirname(OUTPUT_PATH);
  if (!fs.existsSync(outDir)) {
    fs.mkdirSync(outDir, { recursive: true });
  }
  fs.writeFileSync(OUTPUT_PATH, JSON.stringify(merged, null, 2), 'utf8');
  console.log(`Wrote ${merged.length} services to ${OUTPUT_PATH}`);
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
