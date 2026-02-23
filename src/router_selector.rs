use crate::config::Router;
use moka::sync::Cache;
use rand::seq::SliceRandom;
use rand::prelude::Distribution;
use std::time::Duration;
use glob::Pattern;

#[derive(Clone)]
pub struct RouterSelector {
    // Cache key: "router_name:model_name" -> value: Option<TargetChannelName>
    rule_cache: Cache<String, Option<String>>,
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

    /// Find the target channel for a given router and model.
    /// Returns the channel name.
    pub fn select_channel(&self, router: &Router, model: &str) -> Option<String> {
        // Use unified rule-based selection
        // We cache the index of the matched rule, or None if no rule matches
        let cache_key = format!("{}:{}", router.name, model);
        
        let _rule_idx: Option<usize> = if let Some(_idx) = self.rule_cache.get(&cache_key) {
             // Parse cached value: "some(1)" or "none"
             // For simplicity, let's just cache the target channel directly if it's deterministic?
             // But wait, strategy might be random/round_robin, so we can't cache the final channel name.
             // We MUST cache which RULE matched, and then re-apply strategy.
             // However, our current cache stores Option<String>.
             // Let's refactor the cache usage.
             // Since we are changing logic significantly, let's rebuild the flow.
             None // Disable cache for a moment to implement logic first
        } else {
             None
        };
        
        // Find matching rule
        // Note: In a real high-perf scenario, we should cache the matched rule index.
        // For now, let's iterate rules every time or use the cache to store "channel name" IF strategy is priority (deterministic).
        // But if strategy is round_robin, we must re-evaluate.
        
        // Let's just find the rule first.
        let matched_rule = router.rules.iter().find(|rule| {
            for pattern_str in &rule.match_spec.models {
                // 1. Exact match
                if pattern_str == model {
                    return true;
                }
                // 2. Glob match
                if let Ok(pattern) = Pattern::new(pattern_str) {
                    if pattern.matches(model) {
                        return true;
                    }
                }
            }
            false
        });
        
        if let Some(rule) = matched_rule {
             return self.apply_strategy(&rule.channels, &rule.strategy);
        }

        // If no rules matched (shouldn't happen if we have a default * rule, but possible)
        None
    }

    fn apply_strategy(&self, channels: &[crate::config::TargetChannel], strategy: &str) -> Option<String> {
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
            "round_robin" | _ => {
                // Weighted Random as stateless approximation of Weighted Round Robin
                let dist = rand::distributions::WeightedIndex::new(
                    channels.iter().map(|c| c.weight)
                );
                
                match dist {
                    Ok(dist) => {
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
    use crate::config::{Router, RouterRule, MatchSpec, TargetChannel};

    fn create_channel(name: &str, weight: u32) -> TargetChannel {
        TargetChannel {
            name: name.to_string(),
            weight,
        }
    }

    fn create_router(rules: Vec<RouterRule>) -> Router {
        Router {
            name: "test-router".to_string(),
            vkey: None,
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
        let rules = vec![
            RouterRule {
                match_spec: MatchSpec { models: vec!["gpt-4".to_string()] },
                channels: vec![create_channel("ch1", 1), create_channel("ch2", 1)],
                strategy: "priority".to_string(),
            }
        ];
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
        let rules = vec![
            RouterRule {
                match_spec: MatchSpec { models: vec!["gpt-*".to_string()] },
                channels: vec![create_channel("ch1", 1)],
                strategy: "priority".to_string(),
            }
        ];
        let router = create_router(rules);

        assert_eq!(selector.select_channel(&router, "gpt-4"), Some("ch1".to_string()));
        assert_eq!(selector.select_channel(&router, "gpt-3.5"), Some("ch1".to_string()));
        assert_eq!(selector.select_channel(&router, "claude"), None);
    }

    #[test]
    fn test_round_robin_distribution() {
        let selector = RouterSelector::new();
        let rules = vec![
            RouterRule {
                match_spec: MatchSpec { models: vec!["*".to_string()] },
                channels: vec![create_channel("A", 1), create_channel("B", 1)],
                strategy: "round_robin".to_string(),
            }
        ];
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
        let rules = vec![
            RouterRule {
                match_spec: MatchSpec { models: vec!["*".to_string()] },
                channels: vec![create_channel("A", 10), create_channel("B", 0)], // B has 0 weight
                strategy: "round_robin".to_string(),
            }
        ];
        let router = create_router(rules);

        for _ in 0..20 {
            let ch = selector.select_channel(&router, "any").unwrap();
            assert_eq!(ch, "A"); // Should always be A because B has 0 weight
        }
    }
}
