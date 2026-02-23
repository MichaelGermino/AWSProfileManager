# AWS CLI Scraper

Discovers services from the [AWS CLI docs index](https://docs.aws.amazon.com/cli/latest/) (links in `li.toctree-l2`), then scrapes each service’s available commands from the `#available-commands` section and writes merged JSON for the AWS Profile Manager app.

## URL format

- **Index** (service list): `https://docs.aws.amazon.com/cli/latest/` — services are `li.toctree-l2 a.reference.internal` with `href` like `reference/s3/index.html`.
- **Service page**: `https://docs.aws.amazon.com/cli/latest/reference/<service>/` — commands are in `div#available-commands`.

## Usage

From this directory:

```bash
npm install
npm run scrape
```

- **All services**: `node index.js` — fetches the index page, collects all service slugs from the toctree, then scrapes each service and merges with `awsclimockdata.json`.
- **Subset**: `node index.js s3 cognito-idp ec2` — only those slugs are scraped (no index fetch).

Output is written to `../src/renderer/terminal/awsclidata.json`. After scraping, the script merges in `awsclimockdata.json`: for any service that exists in the mock file (e.g. s3, sts, ec2), the scraped entry is replaced with the mock entry (richer descriptions, options, examples; `mocked: true`). All other services stay as scraped data (`mocked: false`). The result is a single merged tree ready to use in the app.

## Rate limiting

About 1.5 seconds between requests to avoid overloading the docs site.
