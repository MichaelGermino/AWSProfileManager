//! Background refresh loop. Port of the timer half of `refreshScheduler.ts` (the pause state lives
//! in auth.rs with the code that trips it).
//!
//! Every tick (60 s; APM_SCHEDULER_TICK_MS in tests) each auto-refresh profile is refreshed when
//! its own interval has passed since the last scheduled refresh, or when its credentials expire
//! within 15 minutes. The scheduler is never interactive: it never opens a browser.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use serde_json::Value;

use crate::core::Core;
use crate::env::parse_js_date_ms;
use crate::sso_org::{is_identity_center, org_key, org_of_profile};

pub const CHECK_INTERVAL_MS: u64 = 60_000;
const REFRESH_THRESHOLD_MINUTES: i64 = 15;

#[derive(Debug, Default)]
pub struct SchedulerState {
    /// A tick still running when the next is due is not overlapped (1.x `tickInFlight`).
    tick_in_flight: AtomicBool,
    started: AtomicBool,
}

impl Core {
    fn should_refresh_by_expiration(&self, expiration: Option<&str>) -> bool {
        let Some(exp) = expiration.filter(|e| !e.is_empty()) else { return true };
        // An Invalid Date compares false in JavaScript (NaN <= x), so it never counts as expiring.
        parse_js_date_ms(exp).is_some_and(|ms| ms <= self.cfg.clock.now_ms() + REFRESH_THRESHOLD_MINUTES * 60_000)
    }

    /// One scheduler pass. Public so the app (and tests) can run one on demand.
    pub async fn scheduler_tick(&self) {
        if self.scheduler.tick_in_flight.swap(true, Ordering::AcqRel) {
            return;
        }
        self.scheduler_tick_inner().await;
        self.scheduler.tick_in_flight.store(false, Ordering::Release);
    }

    async fn scheduler_tick_inner(&self) {
        if self.refresh_paused() {
            return;
        }
        // Locked: nothing can be decrypted. Bailing here also keeps profiles from being marked as
        // freshly refreshed for work that never happened. The next tick after unlocking proceeds.
        if self.is_locked() {
            return;
        }
        // Just saw a network-layer failure (offline/DNS): skip instead of a burst of futile tries.
        if self.should_skip_due_to_recent_network_failure() {
            return;
        }
        let now = self.cfg.clock.now_ms();
        let profiles: Vec<_> =
            self.profiles().all().into_iter().filter(|p| p.get("autoRefresh") == Some(&Value::Bool(true))).collect();
        let mut blocked_orgs: Vec<String> = Vec::new();
        let state = self.auth_state();

        for profile in profiles {
            let s = |k: &str| profile.get(k).and_then(Value::as_str);
            let id = s("id").unwrap_or("").to_string();
            let org = if is_identity_center(s("authType")) { org_of_profile(s("ssoStartUrl"), s("ssoRegion")) } else { None };
            if let Some(o) = &org {
                if blocked_orgs.contains(&org_key(o)) {
                    continue;
                }
            }
            let minutes = profile.get("refreshIntervalMinutes").and_then(Value::as_f64).unwrap_or(60.0);
            let interval_ms = (minutes * 60_000.0) as i64;
            let last_at = state.last_scheduled_refresh_at.lock().unwrap().get(&id).copied().unwrap_or(0);
            let has_last = last_at > 0;
            let interval_elapsed = has_last && now - last_at >= interval_ms;
            let expiration = s("expiration").map(str::to_string);
            let expiring_soon = self.should_refresh_by_expiration(expiration.as_deref());
            // Already asked for this profile's password and not answered: don't prompt every tick.
            // Ask again once its own interval has passed (or sooner if the user refreshes it).
            let waiting_for_user =
                state.awaiting_credentials.lock().unwrap().get(&id).is_some_and(|e| *e == expiration);
            if !interval_elapsed && (!expiring_soon || waiting_for_user) {
                if !has_last {
                    state.last_scheduled_refresh_at.lock().unwrap().insert(id.clone(), now);
                }
                continue;
            }
            let result = self.refresh_profile(&id, false).await;
            state.last_scheduled_refresh_at.lock().unwrap().insert(id.clone(), now);
            if result.is_required() {
                state.awaiting_credentials.lock().unwrap().insert(id.clone(), expiration);
            } else {
                state.awaiting_credentials.lock().unwrap().remove(&id);
            }
            // An SSO session that needs a human is not a failure, and one prompt per org is enough.
            if result.is_sso_login_required() {
                if let Some(o) = &org {
                    blocked_orgs.push(org_key(o));
                }
            }
        }
    }

    /// Start the background loop (once). `tick_ms`: None = every 60 s.
    pub fn start_scheduler(self: &Arc<Self>, tick_ms: Option<u64>) {
        if self.scheduler.started.swap(true, Ordering::AcqRel) {
            return;
        }
        let period = Duration::from_millis(tick_ms.unwrap_or(CHECK_INTERVAL_MS).max(1));
        // Weak: the loop must not keep a core alive after the app (or a test) drops it.
        let weak = Arc::downgrade(self);
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(period);
            interval.tick().await; // setInterval: first run one period after start
            loop {
                interval.tick().await;
                let Some(core) = weak.upgrade() else { break };
                core.scheduler_tick().await;
            }
        });
    }
}
