use crate::config::Router;
use glob::{MatchOptions, Pattern};
use moka::sync::Cache;
use rand::seq::SliceRandom;
use std::time::Duration;

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
        // Use unified rule-based selection
        // We cache the index of the matched rule, or None if no rule matches
        let cache_key = format!("{}:{}", router.name, model);

        let rule_idx: Option<usize> = if let Some(idx) = self.rule_cache.get(&cache_key) {
            idx
        } else {
            // Find matching rule
            let idx = router.rules.iter().position(|rule| {
                for pattern_str in &rule.match_spec.models {
                    // 1. Exact match (case-insensitive)
                    if pattern_str.eq_ignore_ascii_case(model) {
                        return true;
                    }
                    // 2. Glob match (case-insensitive)
                    if Pattern::new(pattern_str).is_ok_and(|pattern| {
                        pattern.matches_with(
                            model,
                            MatchOptions {
                                case_sensitive: false,
                                require_literal_separator: false,
                                require_literal_leading_dot: false,
                            },
                        )
                    }) {
                        return true;
                    }
                }
                false
            });

            // Cache the result (even if None)
            self.rule_cache.insert(cache_key, idx);
            idx
        };

        if let Some(rule) = rule_idx.and_then(|idx| router.rules.get(idx)) {
            return self.apply_strategy(&rule.channels, &rule.strategy);
        }

        // If no rules matched (shouldn't happen if we have a default * rule, but possible)
        None
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
}
