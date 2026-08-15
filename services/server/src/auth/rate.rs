use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Process-local authentication budgets for a personal deployment.
///
/// Limits reset when the process restarts. They are not a public-signup
/// control plane; they only bound a forgotten client or a noisy local script.
pub const CHALLENGE_TTL_SECS: i64 = 60;
pub const MAX_OUTSTANDING_CHALLENGES: i64 = 8;
pub const CHALLENGE_PER_DEVICE_LIMIT: usize = 20;
pub const SESSION_PER_CHALLENGE_LIMIT: usize = 30;
pub const GLOBAL_WINDOW_LIMIT: usize = 60;
pub const RATE_WINDOW: Duration = Duration::from_secs(60);

#[derive(Clone)]
pub struct AuthLimiter {
    inner: Arc<Mutex<Inner>>,
}

struct Inner {
    per_key: HashMap<String, VecDeque<Instant>>,
    global: VecDeque<Instant>,
}

impl AuthLimiter {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(Inner {
                per_key: HashMap::new(),
                global: VecDeque::new(),
            })),
        }
    }

    pub fn allow_challenge(&self, device_key_id: &str) -> bool {
        self.allow(
            &format!("challenge:{device_key_id}"),
            CHALLENGE_PER_DEVICE_LIMIT,
        )
    }

    pub fn allow_session(&self, challenge_id: &str) -> bool {
        self.allow(
            &format!("session:{challenge_id}"),
            SESSION_PER_CHALLENGE_LIMIT,
        )
    }

    fn allow(&self, key: &str, limit: usize) -> bool {
        let Ok(mut inner) = self.inner.lock() else {
            return false;
        };
        let now = Instant::now();
        prune_window(&mut inner.global, now);
        inner.per_key.retain(|_, times| {
            prune_window(times, now);
            !times.is_empty()
        });
        if inner.global.len() >= GLOBAL_WINDOW_LIMIT {
            return false;
        }
        let bucket = inner.per_key.entry(key.to_string()).or_default();
        prune_window(bucket, now);
        if bucket.len() >= limit {
            return false;
        }
        bucket.push_back(now);
        inner.global.push_back(now);
        true
    }
}

impl Default for AuthLimiter {
    fn default() -> Self {
        Self::new()
    }
}

fn prune_window(times: &mut VecDeque<Instant>, now: Instant) {
    while times
        .front()
        .is_some_and(|instant| now.duration_since(*instant) >= RATE_WINDOW)
    {
        times.pop_front();
    }
}
