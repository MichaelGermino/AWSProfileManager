//! AWS CLI reference docs: parses HTML and caches on disk. Port of `awsCliDocsService.ts` and
//! `browserFetchService.ts`.
//! @see https://docs.aws.amazon.com/cli/latest/
//!
//! 1.x fetched the pages in a hidden BrowserWindow (Chromium network stack, system certs, proxy);
//! 2.0 fetches them with the shared HTTP client, which trusts the OS store the same way.

use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::Duration;

use regex::Regex;
use scraper::{Html, Selector};
use serde_json::{json, Value};

use super::collate::locale_compare;
use crate::core::Core;
use crate::env::Platform;
use crate::js;
use crate::json as jsjson;

pub const AWS_CLI_INDEX_URL: &str = "https://docs.aws.amazon.com/cli/latest/";
pub const AWS_CLI_BASE_REFERENCE: &str = "https://docs.aws.amazon.com/cli/latest/reference";
const CACHE_TTL_MS: f64 = 24.0 * 60.0 * 60.0 * 1000.0; // 24 hours

/// The page fetch gives up after this long (`TIMEOUT_MS` in browserFetchService.ts).
const FETCH_TIMEOUT: Duration = Duration::from_millis(20_000);
/// browserFetchService.ts loaded the pages with this browser User-Agent.
const BROWSER_USER_AGENT: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36";

/// Electron's `productName`, which names `app.getPath('userData')`.
const PRODUCT_NAME: &str = "AWS Profile Manager";

/// Parse the index page for service slugs from `li.toctree-l2 > a.reference.internal`.
/// Example: `<li class="toctree-l2"><a class="reference internal" href="reference/partnercentral-channel/index.html">partnercentral-channel</a></li>`
pub fn parse_service_list(html: &str) -> Vec<String> {
    static SLUG: OnceLock<Regex> = OnceLock::new();
    let slug_re = SLUG.get_or_init(|| Regex::new(r"reference/([^/]+)(?:/|$)").unwrap());
    let doc = Html::parse_document(html);
    let sel = Selector::parse(r#"li.toctree-l2 a.reference.internal[href*="reference/"]"#).unwrap();
    let mut slugs: Vec<String> = Vec::new();
    for el in doc.select(&sel) {
        let href = el.value().attr("href").unwrap_or("");
        if let Some(m) = slug_re.captures(href) {
            let raw = &m[1];
            let slug = js::trim(raw.strip_suffix(".html").unwrap_or(raw)).to_string();
            if !slug.is_empty() && !slugs.contains(&slug) {
                slugs.push(slug);
            }
        }
    }
    slugs.sort_by(|a, b| locale_compare(a, b));
    slugs
}

/// `path.basename(p, '.html')` (win32 also splits on backslashes).
fn basename_html(p: &str, platform: Platform) -> String {
    let is_sep = |c: char| c == '/' || (platform == Platform::Windows && c == '\\');
    let trimmed = p.trim_end_matches(is_sep);
    let base = trimmed.rsplit(is_sep).next().unwrap_or("");
    match base.strip_suffix(".html") {
        Some(stem) if !stem.is_empty() => stem.to_string(),
        _ => base.to_string(),
    }
}

/// Parse a service reference page for commands from `li.toctree-l1 > a.reference.internal`.
/// Example: `<li class="toctree-l1"><a class="reference internal" href="mb.html">mb</a></li>`
pub fn parse_commands(html: &str, platform: Platform) -> Vec<(String, String)> {
    static VALID: OnceLock<Regex> = OnceLock::new();
    let valid = VALID.get_or_init(|| Regex::new(r"^[a-z0-9][a-z0-9-]*$").unwrap());
    let doc = Html::parse_document(html);
    let sel = Selector::parse("li.toctree-l1 a.reference.internal[href]").unwrap();
    let mut commands: Vec<(String, String)> = Vec::new();
    for el in doc.select(&sel) {
        let href = js::trim(el.value().attr("href").unwrap_or("")).to_string();
        let text: String = el.text().collect();
        let text = js::trim(&text);
        let name = if text.is_empty() { basename_html(&href, platform) } else { text.to_string() };
        if name.is_empty() || !valid.is_match(&name) || commands.iter().any(|(n, _)| *n == name) {
            continue;
        }
        commands.push((name, href));
    }
    commands
}

/// `serviceSlug.replace(/\/$/, '').replace(/\.html$/, '').trim()`.
pub fn normalize_slug(service_slug: &str) -> String {
    let s = service_slug.strip_suffix('/').unwrap_or(service_slug);
    let s = s.strip_suffix(".html").unwrap_or(s);
    js::trim(s).to_string()
}

/// `Date.now() - data.at < CACHE_TTL_MS` with a truthy `at`.
fn fresh(at: Option<&Value>, now_ms: i64) -> bool {
    let at = match at {
        Some(Value::Number(n)) => n.as_f64().unwrap_or(0.0),
        _ => return false,
    };
    at != 0.0 && !at.is_nan() && (now_ms as f64 - at) < CACHE_TTL_MS
}

impl Core {
    /// `<userData>/aws-cli-docs-cache`, created on first use. 1.x used Electron's `userData`
    /// (named after productName: `%APPDATA%\AWS Profile Manager`), not the app-data directory.
    fn cli_docs_cache_dir(&self) -> PathBuf {
        let app_data = &self.cfg.paths.app_data;
        let user_data = app_data.parent().map(|p| p.join(PRODUCT_NAME)).unwrap_or_else(|| app_data.join(PRODUCT_NAME));
        let dir = user_data.join("aws-cli-docs-cache");
        let _ = std::fs::create_dir_all(&dir);
        dir
    }

    /// The cached service list if it is less than 24 hours old; otherwise `None`.
    pub fn cached_service_list(&self) -> Option<Value> {
        let data = jsjson::read_file(&self.cli_docs_cache_dir().join("services.json"))?;
        let slugs = data.get("slugs").filter(|s| s.is_array())?;
        fresh(data.get("at"), self.cfg.clock.now_ms()).then(|| slugs.clone())
    }

    /// Parse the index HTML and cache it; returns the service slugs.
    pub fn parse_and_cache_service_list(&self, html: &str) -> Result<Value, String> {
        let slugs = parse_service_list(html);
        let path = self.cli_docs_cache_dir().join("services.json");
        let data = json!({ "slugs": slugs, "at": self.cfg.clock.now_ms() });
        std::fs::write(&path, jsjson::stringify(&data)).map_err(|e| e.to_string())?;
        Ok(data["slugs"].clone())
    }

    /// Cached commands for a service if less than 24 hours old; otherwise `None`.
    pub fn cached_commands(&self, service_slug: &str) -> Option<Value> {
        let slug = normalize_slug(service_slug);
        let data = jsjson::read_file(&self.cli_docs_cache_dir().join(format!("{slug}.json")))?;
        let commands = data.get("commands").filter(|s| s.is_array())?;
        fresh(data.get("at"), self.cfg.clock.now_ms()).then(|| commands.clone())
    }

    /// Parse a service page's HTML and cache it; returns the commands (ScrapedCommand[]).
    pub fn parse_and_cache_commands(&self, service_slug: &str, html: &str) -> Result<Value, String> {
        let slug = normalize_slug(service_slug);
        let base_url = format!("{AWS_CLI_BASE_REFERENCE}/{slug}");
        let commands: Vec<Value> = parse_commands(html, self.cfg.platform)
            .into_iter()
            .map(|(name, href)| {
                let doc_url = if href.starts_with("http") { href } else { format!("{base_url}/{}", href.strip_prefix('/').unwrap_or(&href)) };
                json!({
                    "id": format!("{slug}-{name}"),
                    "name": name,
                    "description": "",
                    "syntax": format!("aws {slug} {name} [options]"),
                    "options": [],
                    "examples": [],
                    "mocked": false,
                    "docUrl": doc_url,
                })
            })
            .collect();
        let path = self.cli_docs_cache_dir().join(format!("{slug}.json"));
        let data = json!({ "commands": commands, "at": self.cfg.clock.now_ms() });
        std::fs::write(&path, jsjson::stringify(&data)).map_err(|e| e.to_string())?;
        Ok(data["commands"].clone())
    }

    /// Fetch a docs page's HTML. 1.x loaded it in a hidden BrowserWindow and returned
    /// `document.documentElement.outerHTML` once it finished loading — HTTP error pages included;
    /// only network failures and the 20 s timeout rejected.
    pub async fn fetch_docs_html(&self, url: &str) -> Result<String, String> {
        let client = self.cfg.http.client(false, true);
        let fetch = async {
            let response = client
                .get(self.cfg.http.remap(url))
                .header("User-Agent", BROWSER_USER_AGENT)
                .send()
                .await
                .map_err(|e| load_failed(&e))?;
            response.text().await.map_err(|e| load_failed(&e))
        };
        match tokio::time::timeout(FETCH_TIMEOUT, fetch).await {
            Ok(r) => r,
            Err(_) => Err(format!("Timeout loading {url}")),
        }
    }

    /// The renderer-facing `getAwsCliServiceList()` (preload): cached list, else fetch the index
    /// page, parse and cache it.
    pub async fn aws_cli_service_list(&self) -> Result<Value, String> {
        if let Some(cached) = self.cached_service_list() {
            return Ok(cached);
        }
        let html = self.fetch_docs_html(AWS_CLI_INDEX_URL).await?;
        self.parse_and_cache_service_list(&html)
    }

    /// The renderer-facing `getAwsCliCommandsForService(slug)` (preload): cached commands, else
    /// fetch `<reference>/<slug>/`, parse and cache.
    pub async fn aws_cli_commands_for_service(&self, service_slug: &str) -> Result<Value, String> {
        if let Some(cached) = self.cached_commands(service_slug) {
            return Ok(cached);
        }
        let url = format!("{AWS_CLI_BASE_REFERENCE}/{}/", normalize_slug(service_slug));
        let html = self.fetch_docs_html(&url).await?;
        self.parse_and_cache_commands(service_slug, &html)
    }
}

/// Chromium's `did-fail-load` message: `Load failed: <errorDescription> (<errorCode>)`.
fn load_failed(err: &reqwest::Error) -> String {
    let (desc, code) = if err.is_timeout() {
        ("ERR_TIMED_OUT", -7)
    } else if err.is_connect() {
        let text = format!("{err:?}").to_lowercase();
        if text.contains("dns") || text.contains("resolve") || text.contains("no such host") {
            ("ERR_NAME_NOT_RESOLVED", -105)
        } else if text.contains("certificate") || text.contains("tls") {
            ("ERR_CERT_AUTHORITY_INVALID", -202)
        } else {
            ("ERR_CONNECTION_REFUSED", -102)
        }
    } else if err.is_builder() {
        ("ERR_INVALID_URL", -300)
    } else {
        ("ERR_FAILED", -2)
    };
    format!("Load failed: {desc} ({code})")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_like_cheerio() {
        let index = r#"<ul><li class="toctree-l2"><a class="reference internal" href="reference/s3/index.html">s3</a></li>
            <li class="toctree-l2"><a class="reference internal" href="reference/ec2/index.html">ec2</a></li>
            <li class="toctree-l2"><a class="reference internal" href="reference/s3/index.html">again</a></li>
            <li class="toctree-l2"><a class="reference internal" href="reference/lambda.html">l</a></li>
            <li class="toctree-l1"><a class="reference internal" href="userguide/index.html">User guide</a></li></ul>"#;
        assert_eq!(parse_service_list(index), ["ec2", "lambda", "s3"]);
        let page = r#"<ul><li class="toctree-l1"><a class="reference internal" href="ls.html"> ls </a></li>
            <li class="toctree-l1"><a class="reference internal" href="mb.html"></a></li>
            <li class="toctree-l1"><a class="reference internal" href="ls.html">ls</a></li>
            <li class="toctree-l1"><a class="reference internal" href="wait/index.html">Wait Commands</a></li></ul>"#;
        assert_eq!(parse_commands(page, Platform::Linux), [("ls".to_string(), "ls.html".to_string()), ("mb".to_string(), "mb.html".to_string())]);
        assert_eq!(normalize_slug(" s3.html/"), "s3");
    }
}
