//! "What's new" after an update: release notes and the guided feature tour. Ports of
//! `changelogService.ts`, `tourService.ts` and `devFlags.ts`.
//!
//! Release notes: the GitHub release for the running version is the single source of truth —
//! release notes are written once, where they are already written, and nothing is duplicated into
//! the repo. No notes on the release means no popup: an empty modal is worse than none. Shown at
//! most once per version, tracked by settings.lastChangelogVersionSeen.
//!
//! Tour: the core owns only the *bookkeeping* — which tours have been seen, whether this install
//! has any history, whether the dev flag is on. The tours themselves live in the window because
//! every step is a UI anchor and a route; the core answers "what has been seen", the window
//! answers "what is due".
//!
//! The one exception is the dev flag (see [`is_whats_new_forced`]), which replays both on every
//! launch and prefers `docs/releasenotes/<version>.md` if that file exists — so notes can be
//! written and seen in the running app before the release they will live on is published. That
//! path never records anything as seen, so it cannot consume the real one-shot.

use std::time::Duration;

use serde_json::{json, Value};

use crate::core::Core;
use crate::js;

const GITHUB_OWNER: &str = "MichaelGermino";
const GITHUB_REPO: &str = "AWSProfileManager";
const FETCH_TIMEOUT: Duration = Duration::from_secs(10);

/// Ids of the tours the window ships (`src/renderer/tour/tourSteps.ts` `TOURS`), oldest first.
/// Only used to bank them all on a fresh install; keep in step with the window's tour list.
pub const SHIPPED_TOUR_IDS: &[&str] = &["v1.5-features-ga", "v1.6-features"];

const ENV_KEYS: &[&str] = &["FORCE_WHATS_NEW", "FORCE_TOUR"];
const ARGV_FLAGS: &[&str] = &["--force-whats-new", "--force-tour"];

/// Developer switch for the post-update "what's new" surfaces (`devFlags.ts`).
///
/// The release-notes modal and the guided feature tour both show at most once per install, which
/// makes them nearly impossible to iterate on: you get one shot per version, on a machine that has
/// already taken it. This flag replays both on every launch without touching the stored state, so
/// the real trigger path stays exactly as it ships.
///
/// Two ways to set it: `FORCE_WHATS_NEW=1` in the environment (the dev loop), or
/// `--force-whats-new` on the command line (a packaged build). It is deliberately NOT gated on a
/// release build: verifying this against the installer is the whole point, and the flag only
/// replays UI that every user sees anyway — it reveals nothing and writes nothing.
///
/// Read live rather than cached so a test can set the env var late.
pub fn is_whats_new_forced() -> bool {
    if ENV_KEYS.iter().any(|k| std::env::var(k).as_deref() == Ok("1")) {
        return true;
    }
    std::env::args().any(|a| ARGV_FLAGS.contains(&a.as_str()))
}

fn release_page(version: &str) -> String {
    format!("https://github.com/{GITHUB_OWNER}/{GITHUB_REPO}/releases/tag/v{version}")
}

enum Fetch {
    Ok(Value),
    /// `missing`: 404, no published release for this tag (settled); otherwise a transient failure.
    Failed {
        missing: bool,
    },
}

impl Core {
    fn seen_tour_ids(&self) -> Vec<Value> {
        self.settings().get().get("seenTourIds").and_then(Value::as_array).cloned().unwrap_or_default()
    }

    fn has_no_history(&self, settings: &crate::store::Obj) -> bool {
        settings.get("setupCompleted") != Some(&Value::Bool(true)) && self.profiles().all().is_empty()
    }

    /// Release for a version tag, or a failure. A failure never marks the version as seen unless
    /// it is a 404.
    async fn fetch_release(&self, version: &str) -> Fetch {
        // Tags carry the v prefix; the version in package.json does not.
        let url = format!("https://api.github.com/repos/{GITHUB_OWNER}/{GITHUB_REPO}/releases/tags/v{}", super::encode_uri_component(version));
        let client = self.cfg.http.client(false, true);
        // GitHub rejects requests without a User-Agent; the shared client always sends one.
        let sent = client.get(self.cfg.http.remap(&url)).header("accept", "application/vnd.github+json").timeout(FETCH_TIMEOUT).send().await;
        let Ok(res) = sent else { return Fetch::Failed { missing: false } };
        let status = res.status().as_u16();
        if status != 200 {
            // 404 means no published release for this tag — a dev build, or a draft. Settled, not
            // a transient failure, so let the caller stop asking.
            return Fetch::Failed { missing: status == 404 };
        }
        match res.text().await.ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()) {
            Some(v) => Fetch::Ok(v),
            None => Fetch::Failed { missing: false },
        }
    }

    /// Notes checked into the repo for this version, or `None`.
    ///
    /// Only reachable in a dev tree (`docs/` next to `resources/`): an installed build has no
    /// `docs/`, so this always misses and the GitHub fetch is the only source — exactly as it ships.
    fn read_local_notes(&self, version: &str) -> Option<String> {
        let file = self.cfg.resources_dir.parent()?.join("docs").join("releasenotes").join(format!("{version}.md"));
        let body = std::fs::read_to_string(file).ok()?;
        let body = js::trim(&body);
        (!body.is_empty()).then(|| body.to_string())
    }

    /// `markChangelogSeen`: record a version as shown, so it never appears again. A no-op under
    /// the dev flag.
    pub fn mark_changelog_seen(&self, version: &str) {
        if is_whats_new_forced() {
            return;
        }
        let mut settings = self.settings().get();
        if settings.get("lastChangelogVersionSeen").and_then(Value::as_str) == Some(version) {
            return;
        }
        settings.insert("lastChangelogVersionSeen".into(), version.into());
        self.settings().save(&settings);
    }

    /// A first-ever launch is not an update. Nothing recorded, no profiles and setup not finished
    /// means this install has never run, and that user should meet the setup wizard rather than
    /// release notes for a version they have no history with.
    fn changelog_looks_like_fresh_install(&self, settings: &crate::store::Obj) -> bool {
        let seen = settings.get("lastChangelogVersionSeen");
        let nothing_recorded = match seen {
            None | Some(Value::Null) => true,
            Some(Value::String(s)) => s.is_empty(),
            Some(Value::Bool(b)) => !b,
            Some(Value::Number(n)) => n.as_f64() == Some(0.0),
            _ => false,
        };
        nothing_recorded && self.has_no_history(settings)
    }

    /// `getPendingChangelog`: release notes to show now, or `None`.
    ///
    /// Marks the version seen for every *settled* outcome — shown, no notes, no release — so a
    /// launch costs at most one API call per version. A network failure is deliberately NOT
    /// settled: it stays pending and is retried next launch, because "offline right now" should
    /// not silently consume the one chance to show what changed.
    pub async fn pending_changelog(&self) -> Option<Value> {
        let version = self.cfg.app_version.clone();
        let settings = self.settings().get();
        let forced = is_whats_new_forced();

        if forced {
            // Repo notes first so unreleased versions have something to show; GitHub still answers
            // for a version whose .md was never written.
            if let Some(local) = self.read_local_notes(&version) {
                return Some(json!({ "version": version, "notes": local, "url": release_page(&version) }));
            }
        } else if settings.get("lastChangelogVersionSeen").and_then(Value::as_str) == Some(version.as_str()) {
            return None;
        }

        if self.changelog_looks_like_fresh_install(&settings) && !forced {
            self.mark_changelog_seen(&version);
            return None;
        }

        let release = match self.fetch_release(&version).await {
            Fetch::Ok(r) => r,
            Fetch::Failed { missing } => {
                if missing {
                    self.mark_changelog_seen(&version);
                }
                return None;
            }
        };

        let notes = js::trim(release.get("body").and_then(Value::as_str).unwrap_or("")).to_string();
        if notes.is_empty() {
            self.mark_changelog_seen(&version);
            return None;
        }
        let url = release.get("html_url").and_then(Value::as_str).map(str::to_string).unwrap_or_else(|| release_page(&version));
        Some(json!({ "version": version, "notes": notes, "url": url }))
    }

    /// `getTourState`.
    pub fn tour_state(&self) -> Value {
        let settings = self.settings().get();
        let seen = settings.get("seenTourIds").filter(|v| v.is_array()).cloned().unwrap_or_else(|| json!([]));
        json!({
            "seenTourIds": seen,
            "forced": is_whats_new_forced(),
            // This install has never been used: setup not finished and no profiles. A first-ever
            // launch is not an update — that user meets the setup wizard, not a tour of features
            // they have no history with. Deliberately does NOT consult lastChangelogVersionSeen
            // (written on the same launch) so the two checks cannot race.
            "freshInstall": self.has_no_history(&settings),
            "version": self.cfg.app_version,
        })
    }

    /// `markTourSeen`: record a tour as shown, so it never appears again. A no-op under the dev flag.
    pub fn mark_tour_seen(&self, id: &str) {
        if is_whats_new_forced() {
            return;
        }
        let mut seen = self.seen_tour_ids();
        if seen.iter().any(|v| v.as_str() == Some(id)) {
            return;
        }
        seen.push(id.into());
        let mut settings = self.settings().get();
        settings.insert("seenTourIds".into(), Value::Array(seen));
        self.settings().save(&settings);
    }

    /// `resetWhatsNewState`: forget every tour and the release-notes marker, so the next launch
    /// takes the real first-run-after-update path. This is what the dev flag cannot test: the flag
    /// bypasses the trigger, this exercises it. Exposed from Settings → Advanced → Developer options.
    pub fn reset_whats_new_state(&self) {
        let mut settings = self.settings().get();
        settings.shift_remove("lastChangelogVersionSeen");
        settings.shift_remove("seenTourIds");
        self.settings().save(&settings);
    }

    /// The startup part of the 1.x window's what's-new checks that needs no user: on a fresh
    /// install `getPendingChangelog` records the version without asking GitHub, and the tour check
    /// banks every shipped tour (`App.tsx`: "bank every tour now rather than ambushing them next
    /// launch"). In that order, as the window's effects ran.
    pub(crate) fn settle_whats_new_for_fresh_install(&self) {
        if is_whats_new_forced() {
            return;
        }
        let settings = self.settings().get();
        if settings.get("lastChangelogVersionSeen").and_then(Value::as_str) != Some(self.cfg.app_version.as_str())
            && self.changelog_looks_like_fresh_install(&settings)
        {
            self.mark_changelog_seen(&self.cfg.app_version.clone());
        }
        if self.has_no_history(&self.settings().get()) {
            for id in SHIPPED_TOUR_IDS {
                self.mark_tour_seen(id);
            }
        }
    }
}
