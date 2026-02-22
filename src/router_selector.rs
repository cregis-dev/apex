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
        // 1. Try to find a rule-based match (cached)
        // We use the cache to store the result of the rule matching process.
        // If the rule matching returns None (no rule matched), we store None in the cache.
        // This avoids re-scanning the rules for every request.
        let rule_target = self.find_rule_match(router, model);
        
        if let Some(target) = rule_target {
            return Some(target);
        }

        // 2. If no rule matched, use the router's strategy to pick from channels
        self.apply_strategy(router)
    }

    fn find_rule_match(&self, router: &Router, model: &str) -> Option<String> {
        let cache_key = format!("{}:{}", router.name, model);
        
        if let Some(cached) = self.rule_cache.get(&cache_key) {
            return cached;
        }

        let target = self.match_rule_internal(router, model);
        self.rule_cache.insert(cache_key, target.clone());
        target
    }

    fn match_rule_internal(&self, router: &Router, model: &str) -> Option<String> {
        let metadata = router.metadata.as_ref()?;
        
        // 1. Exact match
        if let Some(target) = metadata.model_matcher.get(model) {
            return Some(target.clone());
        }

        // 2. Glob match
        for (pattern_str, target) in &metadata.model_matcher {
            if let Ok(pattern) = Pattern::new(pattern_str) {
                if pattern.matches(model) {
                    return Some(target.clone());
                }
            }
        }

        None
    }

    fn apply_strategy(&self, router: &Router) -> Option<String> {
        if router.channels.is_empty() {
            return None;
        }

        match router.strategy.as_str() {
            "random" => {
                let mut rng = rand::thread_rng();
                router.channels.choose(&mut rng).map(|c| c.name.clone())
            }
            "priority" => {
                // Always pick the first one
                router.channels.first().map(|c| c.name.clone())
            }
            "round_robin" | _ => {
                // Weighted Random as stateless approximation of Weighted Round Robin
                let dist = rand::distributions::WeightedIndex::new(
                    router.channels.iter().map(|c| c.weight)
                );
                
                match dist {
                    Ok(dist) => {
                        let mut rng = rand::thread_rng();
                        let idx = dist.sample(&mut rng);
                        router.channels.get(idx).map(|c| c.name.clone())
                    }
                    Err(_) => {
                        // Fallback if weights are invalid
                        router.channels.first().map(|c| c.name.clone())
                    }
                }
            }
        }
    }
}
