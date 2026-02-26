use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Instant;

struct TokenBucket {
    tokens: f64,
    last_refill: Instant,
    capacity: f64,
    refill_rate: f64, // tokens per second
}

impl TokenBucket {
    fn new(capacity: f64, refill_rate: f64) -> Self {
        Self {
            tokens: capacity,
            last_refill: Instant::now(),
            capacity,
            refill_rate,
        }
    }

    fn consume(&mut self, amount: f64) -> bool {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_refill).as_secs_f64();
        self.tokens = (self.tokens + elapsed * self.refill_rate).min(self.capacity);
        self.last_refill = now;

        if self.tokens >= amount {
            self.tokens -= amount;
            true
        } else {
            false
        }
    }
}

pub struct TeamRateLimiter {
    // Map<TeamID, Map<Type, Bucket>>
    // Type: "rpm", "tpm"
    buckets: Mutex<HashMap<String, HashMap<String, TokenBucket>>>,
}

impl Default for TeamRateLimiter {
    fn default() -> Self {
        Self::new()
    }
}

impl TeamRateLimiter {
    pub fn new() -> Self {
        Self {
            buckets: Mutex::new(HashMap::new()),
        }
    }

    pub fn check(
        &self,
        team_id: &str,
        rpm_limit: Option<u32>,
        tpm_limit: Option<u32>,
        estimated_tokens: u32,
    ) -> bool {
        let mut buckets = self.buckets.lock().unwrap();
        let team_buckets = buckets.entry(team_id.to_string()).or_default();

        // Check RPM
        if let Some(rpm) = rpm_limit.filter(|&r| r > 0) {
            let bucket = team_buckets
                .entry("rpm".to_string())
                .or_insert_with(|| TokenBucket::new(rpm as f64, rpm as f64 / 60.0));

            // Update rate if config changed
            if (bucket.capacity - rpm as f64).abs() > 0.1 {
                *bucket = TokenBucket::new(rpm as f64, rpm as f64 / 60.0);
            }

            if !bucket.consume(1.0) {
                return false;
            }
        }

        // Check TPM (estimated)
        if let Some(tpm) = tpm_limit.filter(|&t| t > 0) {
            let bucket = team_buckets
                .entry("tpm".to_string())
                .or_insert_with(|| TokenBucket::new(tpm as f64, tpm as f64 / 60.0));

            if (bucket.capacity - tpm as f64).abs() > 0.1 {
                *bucket = TokenBucket::new(tpm as f64, tpm as f64 / 60.0);
            }

            if !bucket.consume(estimated_tokens as f64) {
                return false;
            }
        }

        true
    }
}
