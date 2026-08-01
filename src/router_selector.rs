use crate::config::Router;
use glob::{MatchOptions, Pattern};
use moka::sync::Cache;
use rand::seq::SliceRandom;
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouteSelection {
    pub channel_name: String,
    pub matched_rule: Option<String>,
}

/// An ordered list of candidate channels for a request. The first entry is the
/// primary; the rest are in-rule failovers, tried in order before the router's
/// `fallback_channels`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouteCandidates {
    pub channels: Vec<String>,
    pub matched_rule: Option<String>,
}

#[derive(Clone)]
pub struct RouterSelector {
    // Cache key: "router_name:model_name" -> value: Option<usize> (index of matched rule)
    rule_cache: Cache<String, Option<usize>>,
}

impl Default for RouterSelector {
    fn default() -> Self {
        Self::new()
    }
}

impl RouterSelector {
    pub fn new() -> Self {
        Self {
            rule_cache: Cache::builder()
                .max_capacity(10_000)
                .time_to_live(Duration::from_secs(3600)) // 1 hour TTL
                .build(),
        }
    }

    /// Invalidate the rule cache.
    /// Should be called when configuration is reloaded.
    pub fn invalidate_cache(&self) {
        self.rule_cache.invalidate_all();
    }

    /// Find the target channel for a given router and model.
    /// Returns the channel name.
    pub fn select_channel(&self, router: &Router, model: &str) -> Option<String> {
        self.select_channel_with_rule(router, model)
            .map(|selection| selection.channel_name)
    }

    /// Find the target channel and matched rule descriptor for a given router/model pair.
    pub fn select_channel_with_rule(&self, router: &Router, model: &str) -> Option<RouteSelection> {
        let rule = self
            .matched_rule_index(router, model)
            .and_then(|idx| router.rules.get(idx))?;
        self.apply_strategy(&rule.channels, &rule.strategy)
            .map(|channel_name| RouteSelection {
                channel_name,
                matched_rule: Some(Self::describe_rule(rule)),
            })
    }

    /// Resolve an ordered list of candidate channels for a router/model pair.
    ///
    /// Unlike [`select_channel_with_rule`], which returns a single primary, this
    /// returns the whole matched rule's channels ordered by strategy so the
    /// caller can fail over *within the rule* before touching the router's
    /// `fallback_channels`:
    ///   * `priority`    — config order (top = primary, rest are failovers).
    ///   * `random`      — shuffled; or a deterministic primary when the rule
    ///     opts into session affinity and `sticky_key` is set.
    ///   * `round_robin` — weighted-random primary then the rest; or a
    ///     deterministic weighted primary when sticky.
    ///
    /// `sticky_key` is a conversation-stable identifier (see
    /// [`crate::request_hash::session_key`]); it only matters when the matched
    /// rule sets `session_affinity`.
    pub fn select_candidates(
        &self,
        router: &Router,
        model: &str,
        sticky_key: Option<&str>,
    ) -> Option<RouteCandidates> {
        let rule = self
            .matched_rule_index(router, model)
            .and_then(|idx| router.rules.get(idx))?;
        // Affinity only applies to the non-deterministic strategies; `priority`
        // ignores it because it is already deterministic.
        let effective_key = if rule.session_affinity {
            sticky_key
        } else {
            None
        };
        let channels = order_channels(&rule.channels, &rule.strategy, effective_key);
        if channels.is_empty() {
            return None;
        }
        Some(RouteCandidates {
            channels,
            matched_rule: Some(Self::describe_rule(rule)),
        })
    }

    /// Index of the first rule whose match spec matches `model`, memoized per
    /// `router:model`. A cached `None` means no rule matched.
    fn matched_rule_index(&self, router: &Router, model: &str) -> Option<usize> {
        let cache_key = format!("{}:{}", router.name, model);
        if let Some(idx) = self.rule_cache.get(&cache_key) {
            return idx;
        }
        let idx = router.rules.iter().position(|rule| {
            rule.match_spec.models.iter().any(|pattern_str| {
                // Exact (case-insensitive) or glob match.
                pattern_str.eq_ignore_ascii_case(model)
                    || Pattern::new(pattern_str).is_ok_and(|pattern| {
                        pattern.matches_with(
                            model,
                            MatchOptions {
                                case_sensitive: false,
                                require_literal_separator: false,
                                require_literal_leading_dot: false,
                            },
                        )
                    })
            })
        });
        self.rule_cache.insert(cache_key, idx);
        idx
    }

    fn describe_rule(rule: &crate::config::RouterRule) -> String {
        if rule.match_spec.models.is_empty() {
            return "*".to_string();
        }

        rule.match_spec.models.join(" | ")
    }

    fn apply_strategy(
        &self,
        channels: &[crate::config::TargetChannel],
        strategy: &str,
    ) -> Option<String> {
        if channels.is_empty() {
            return None;
        }

        match strategy {
            "random" => {
                let mut rng = rand::thread_rng();
                channels.choose(&mut rng).map(|c| c.name.clone())
            }
            "priority" => {
                // Always pick the first one
                channels.first().map(|c| c.name.clone())
            }
            "round_robin" => {
                let dist =
                    rand::distributions::WeightedIndex::new(channels.iter().map(|c| c.weight));

                match dist {
                    Ok(dist) => {
                        use rand::distributions::Distribution;
                        let mut rng = rand::thread_rng();
                        let idx = dist.sample(&mut rng);
                        channels.get(idx).map(|c| c.name.clone())
                    }
                    Err(_) => {
                        // Fallback if weights are invalid
                        channels.first().map(|c| c.name.clone())
                    }
                }
            }
            _ => {
                // Default to round robin if unknown
                let dist =
                    rand::distributions::WeightedIndex::new(channels.iter().map(|c| c.weight));

                match dist {
                    Ok(dist) => {
                        use rand::distributions::Distribution;
                        let mut rng = rand::thread_rng();
                        let idx = dist.sample(&mut rng);
                        channels.get(idx).map(|c| c.name.clone())
                    }
                    Err(_) => {
                        // Fallback if weights are invalid
                        channels.first().map(|c| c.name.clone())
                    }
                }
            }
        }
    }
}

/// Deterministic 64-bit hash of a session key. blake3 is stable across runs and
/// versions, so a conversation keeps its channel even across process restarts.
fn hash_to_u64(key: &str) -> u64 {
    let digest = blake3::hash(key.as_bytes());
    let bytes = digest.as_bytes();
    u64::from_le_bytes(bytes[..8].try_into().expect("blake3 digest is 32 bytes"))
}

/// Map `key` into the cumulative-weight space so a session lands on a stable
/// channel while weight still biases the spread across sessions. A zero-weight
/// channel is never picked; if every weight is zero, fall back to a uniform
/// hash over indices.
fn weighted_index_for_key(channels: &[&crate::config::TargetChannel], key: &str) -> usize {
    let total: u64 = channels.iter().map(|c| c.weight as u64).sum();
    if total == 0 {
        return (hash_to_u64(key) % channels.len().max(1) as u64) as usize;
    }
    let mut point = hash_to_u64(key) % total;
    for (i, c) in channels.iter().enumerate() {
        let w = c.weight as u64;
        if point < w {
            return i;
        }
        point -= w;
    }
    channels.len() - 1
}

/// Weighted-random index — the non-sticky `round_robin` primary pick. Honors
/// zero weights (a 0-weight channel is never chosen); errors (all-zero/empty)
/// degrade to the first channel.
fn weighted_random_index(channels: &[&crate::config::TargetChannel]) -> usize {
    use rand::distributions::{Distribution, WeightedIndex};
    match WeightedIndex::new(channels.iter().map(|c| c.weight)) {
        Ok(dist) => dist.sample(&mut rand::thread_rng()),
        Err(_) => 0,
    }
}

/// Channel names with `first` at the head, then the remaining channels in config
/// order — the failover chain behind a chosen primary.
fn rotate_first(channels: &[crate::config::TargetChannel], first: usize) -> Vec<String> {
    let mut out = Vec::with_capacity(channels.len());
    if let Some(primary) = channels.get(first) {
        out.push(primary.name.clone());
    }
    for (i, c) in channels.iter().enumerate() {
        if i != first {
            out.push(c.name.clone());
        }
    }
    out
}

/// Order a rule's channels into a candidate list per strategy (see
/// [`RouterSelector::select_candidates`]). The first entry is the primary; the
/// rest are ordered in-rule failovers.
fn order_channels(
    channels: &[crate::config::TargetChannel],
    strategy: &str,
    sticky_key: Option<&str>,
) -> Vec<String> {
    if channels.is_empty() {
        return Vec::new();
    }
    match strategy {
        // Deterministic: top channel is primary, the rest are failovers. Weight
        // and session affinity are irrelevant.
        "priority" => channels.iter().map(|c| c.name.clone()).collect(),
        // Uniform selection — weight is ignored (matches non-sticky `random`).
        // Sticky pins the primary via a hash of the session key.
        "random" => match sticky_key {
            Some(key) => rotate_first(
                channels,
                (hash_to_u64(key) % channels.len() as u64) as usize,
            ),
            None => {
                let mut names: Vec<String> = channels.iter().map(|c| c.name.clone()).collect();
                names.shuffle(&mut rand::thread_rng());
                names
            }
        },
        // round_robin and any unknown strategy: weight-aware. A zero-weight
        // channel is disabled — dropped from the candidate list entirely, unless
        // that would leave nothing to route to.
        _ => {
            let active: Vec<&crate::config::TargetChannel> =
                channels.iter().filter(|c| c.weight > 0).collect();
            let pool: Vec<&crate::config::TargetChannel> = if active.is_empty() {
                channels.iter().collect()
            } else {
                active
            };
            let first = match sticky_key {
                Some(key) => weighted_index_for_key(&pool, key),
                None => weighted_random_index(&pool),
            };
            let mut out = Vec::with_capacity(pool.len());
            if let Some(primary) = pool.get(first) {
                out.push(primary.name.clone());
            }
            for (i, c) in pool.iter().enumerate() {
                if i != first {
                    out.push(c.name.clone());
                }
            }
            out
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{MatchSpec, Router, RouterRule, TargetChannel};

    fn create_channel(name: &str, weight: u32) -> TargetChannel {
        TargetChannel {
            name: name.to_string(),
            weight,
        }
    }

    fn create_router(rules: Vec<RouterRule>) -> Router {
        Router {
            name: "test-router".to_string(),
            rules,
            channels: vec![],
            strategy: "round_robin".to_string(),
            metadata: None,
            fallback_channels: vec![],
        }
    }

    #[test]
    fn test_exact_match_priority() {
        let selector = RouterSelector::new();
        let rules = vec![RouterRule {
            session_affinity: false,
            match_spec: MatchSpec {
                models: vec!["gpt-4".to_string()],
            },
            channels: vec![create_channel("ch1", 1), create_channel("ch2", 1)],
            strategy: "priority".to_string(),
        }];
        let router = create_router(rules);

        // Should always pick ch1
        for _ in 0..10 {
            let ch = selector.select_channel(&router, "gpt-4");
            assert_eq!(ch, Some("ch1".to_string()));
        }

        // Non-matching model
        let ch = selector.select_channel(&router, "gpt-3.5");
        assert_eq!(ch, None);
    }

    #[test]
    fn test_glob_match() {
        let selector = RouterSelector::new();
        let rules = vec![RouterRule {
            session_affinity: false,
            match_spec: MatchSpec {
                models: vec!["gpt-*".to_string()],
            },
            channels: vec![create_channel("ch1", 1)],
            strategy: "priority".to_string(),
        }];
        let router = create_router(rules);

        assert_eq!(
            selector.select_channel(&router, "gpt-4"),
            Some("ch1".to_string())
        );
        assert_eq!(
            selector.select_channel(&router, "gpt-3.5"),
            Some("ch1".to_string())
        );
        assert_eq!(selector.select_channel(&router, "claude"), None);
    }

    #[test]
    fn test_round_robin_distribution() {
        let selector = RouterSelector::new();
        let rules = vec![RouterRule {
            session_affinity: false,
            match_spec: MatchSpec {
                models: vec!["*".to_string()],
            },
            channels: vec![create_channel("A", 1), create_channel("B", 1)],
            strategy: "round_robin".to_string(),
        }];
        let router = create_router(rules);

        let mut counts = std::collections::HashMap::new();
        for _ in 0..100 {
            let ch = selector.select_channel(&router, "any").unwrap();
            *counts.entry(ch).or_insert(0) += 1;
        }

        // With 100 samples, both should be selected roughly 50 times.
        // It's probabilistic, but ensuring both are selected is a basic check.
        assert!(counts.get("A").unwrap() > &0);
        assert!(counts.get("B").unwrap() > &0);
    }

    #[test]
    fn test_weighted_round_robin() {
        let selector = RouterSelector::new();
        let rules = vec![RouterRule {
            session_affinity: false,
            match_spec: MatchSpec {
                models: vec!["*".to_string()],
            },
            channels: vec![create_channel("A", 10), create_channel("B", 0)], // B has 0 weight
            strategy: "round_robin".to_string(),
        }];
        let router = create_router(rules);

        for _ in 0..20 {
            let ch = selector.select_channel(&router, "any").unwrap();
            assert_eq!(ch, "A"); // Should always be A because B has 0 weight
        }
    }

    #[test]
    fn test_case_insensitive_match() {
        let selector = RouterSelector::new();
        let rules = vec![RouterRule {
            session_affinity: false,
            match_spec: MatchSpec {
                models: vec!["GPT-4".to_string()],
            },
            channels: vec![create_channel("ch1", 1)],
            strategy: "priority".to_string(),
        }];
        let router = create_router(rules);

        // Exact match with different case
        assert_eq!(
            selector.select_channel(&router, "gpt-4"),
            Some("ch1".to_string())
        );

        // Glob match with different case
        let rules_glob = vec![RouterRule {
            session_affinity: false,
            match_spec: MatchSpec {
                models: vec!["GPT-*".to_string()],
            },
            channels: vec![create_channel("ch2", 1)],
            strategy: "priority".to_string(),
        }];
        let router_glob = create_router(rules_glob);

        assert_eq!(
            selector.select_channel(&router_glob, "gpt-3.5"),
            Some("ch2".to_string())
        );
    }

    fn rule_with(
        models: &[&str],
        channels: Vec<TargetChannel>,
        strategy: &str,
        session_affinity: bool,
    ) -> RouterRule {
        RouterRule {
            match_spec: MatchSpec {
                models: models.iter().map(|s| s.to_string()).collect(),
            },
            channels,
            strategy: strategy.to_string(),
            session_affinity,
        }
    }

    #[test]
    fn candidates_priority_are_config_order_for_failover() {
        let selector = RouterSelector::new();
        let router = create_router(vec![rule_with(
            &["*"],
            vec![
                create_channel("A", 1),
                create_channel("B", 1),
                create_channel("C", 1),
            ],
            "priority",
            false,
        )]);
        // Full ordered failover chain, primary first.
        let cands = selector.select_candidates(&router, "any", None).unwrap();
        assert_eq!(cands.channels, vec!["A", "B", "C"]);
    }

    #[test]
    fn candidates_round_robin_include_all_channels_as_failovers() {
        let selector = RouterSelector::new();
        let router = create_router(vec![rule_with(
            &["*"],
            vec![create_channel("A", 1), create_channel("B", 1)],
            "round_robin",
            false,
        )]);
        // Whichever is primary, both channels must be present so failover works.
        for _ in 0..50 {
            let cands = selector.select_candidates(&router, "any", None).unwrap();
            assert_eq!(cands.channels.len(), 2);
            let mut sorted = cands.channels.clone();
            sorted.sort();
            assert_eq!(sorted, vec!["A".to_string(), "B".to_string()]);
        }
    }

    #[test]
    fn candidates_round_robin_drop_zero_weight_channels() {
        let selector = RouterSelector::new();
        let router = create_router(vec![rule_with(
            &["*"],
            vec![create_channel("A", 1), create_channel("B", 0)],
            "round_robin",
            false,
        )]);
        // B is disabled (weight 0) — never a candidate, not even as failover.
        for _ in 0..50 {
            let cands = selector.select_candidates(&router, "any", None).unwrap();
            assert_eq!(cands.channels, vec!["A"]);
        }
    }

    #[test]
    fn sticky_pins_a_session_to_one_primary() {
        let selector = RouterSelector::new();
        let router = create_router(vec![rule_with(
            &["*"],
            vec![
                create_channel("A", 1),
                create_channel("B", 1),
                create_channel("C", 1),
            ],
            "round_robin",
            true,
        )]);
        // Same session key ⇒ same primary on every call (prompt-cache affinity).
        let first = selector
            .select_candidates(&router, "any", Some("session-xyz"))
            .unwrap()
            .channels[0]
            .clone();
        for _ in 0..100 {
            let cands = selector
                .select_candidates(&router, "any", Some("session-xyz"))
                .unwrap();
            assert_eq!(cands.channels[0], first);
            // The full failover chain is still present.
            assert_eq!(cands.channels.len(), 3);
        }
    }

    #[test]
    fn sticky_spreads_distinct_sessions_across_channels() {
        let selector = RouterSelector::new();
        let router = create_router(vec![rule_with(
            &["*"],
            vec![create_channel("A", 1), create_channel("B", 1)],
            "round_robin",
            true,
        )]);
        // Different session keys should not all collapse onto one channel.
        let mut seen = std::collections::HashSet::new();
        for i in 0..50 {
            let key = format!("session-{i}");
            let cands = selector
                .select_candidates(&router, "any", Some(&key))
                .unwrap();
            seen.insert(cands.channels[0].clone());
        }
        assert_eq!(seen.len(), 2, "both channels should host some sessions");
    }

    #[test]
    fn affinity_disabled_ignores_sticky_key() {
        let selector = RouterSelector::new();
        // Same channels, but the rule does NOT opt into affinity. A sticky key
        // is passed but must be ignored — priority is still deterministic here,
        // so we just assert the key does not change the (already deterministic)
        // ordering and that non-affinity rules accept the key without error.
        let router = create_router(vec![rule_with(
            &["*"],
            vec![create_channel("A", 1), create_channel("B", 1)],
            "priority",
            false,
        )]);
        let cands = selector
            .select_candidates(&router, "any", Some("session-xyz"))
            .unwrap();
        assert_eq!(cands.channels, vec!["A", "B"]);
    }
}
