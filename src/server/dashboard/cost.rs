use crate::config::Channel;
use crate::database::{RollupRow, UsageRecord as DashboardUsageRecord};

use super::sections::percent_change;
use super::*;

/// Reference (list PAYG) cost of one record under a resolved pricing rule: the
/// rule's rate-card row matching the record's model. Subscriptions have no
/// per-token rates (cost 0 here — the fee is handled separately). Cache tokens
/// are priced separately from input.
pub(super) fn reference_cost_of(
    rec: &DashboardUsageRecord,
    rule: &crate::config::PricingRule,
    unit: f64,
) -> f64 {
    if rule.is_subscription() {
        return 0.0;
    }
    match rule.price_for(&rec.model) {
        Some(p) => {
            (p.input * rec.input_tokens.max(0) as f64
                + p.output * rec.output_tokens.max(0) as f64
                + p.cache_read_rate() * rec.cache_read_tokens.max(0) as f64
                + p.cache_write_rate() * rec.cache_write_tokens.max(0) as f64)
                / unit.max(1.0)
        }
        None => 0.0,
    }
}

/// Build the cost section: each record bills under the rule its channel selects.
/// PAYG rules charge the reference cost; subscription rules split an accrued
/// monthly fee across their traffic by reference share. Records whose channel
/// selects no (or an unknown) rule are untracked and excluded.
pub(super) fn build_cost_section(
    current: &[DashboardUsageRecord],
    previous: &[DashboardUsageRecord],
    pricing: &crate::config::Pricing,
    channels: &[Channel],
    teams: &[crate::config::Team],
    window_secs: f64,
    // When the query is filtered (by channel/user/model/…), only subscriptions with
    // traffic in the filtered window count — otherwise another channel's monthly fee
    // would leak into a scoped view. Unfiltered keeps all rules (idle ones = waste).
    filtered: bool,
) -> DashboardCostSection {
    use std::collections::HashMap;

    let unit = pricing.unit;
    // channel name -> selected rule name
    let channel_rule: HashMap<&str, &str> = channels
        .iter()
        .filter_map(|ch| ch.pricing.as_deref().map(|r| (ch.name.as_str(), r)))
        .collect();
    // rule name -> rule
    let rules: HashMap<&str, &crate::config::PricingRule> =
        pricing.rules.iter().map(|r| (r.name.as_str(), r)).collect();
    let group_by_member: HashMap<&str, Option<String>> = teams
        .iter()
        .map(|t| (t.id.as_str(), t.group.clone()))
        .collect();

    // The rule a record bills under (via its channel), if any.
    let rule_for = |rec: &DashboardUsageRecord| -> Option<&crate::config::PricingRule> {
        channel_rule
            .get(rec.channel.as_str())
            .and_then(|name| rules.get(name).copied())
    };

    // Subscription rules in scope: rule name -> rule.
    // - filtered view: only rules with traffic in the (already-filtered) window;
    // - full view: every rule referenced by a channel (idle ones show as waste).
    let sub_rules: HashMap<&str, &crate::config::PricingRule> = if filtered {
        current
            .iter()
            .filter_map(&rule_for)
            .filter(|r| r.is_subscription())
            .map(|r| (r.name.as_str(), r))
            .collect()
    } else {
        channel_rule
            .values()
            .filter_map(|name| {
                rules
                    .get(name)
                    .copied()
                    .filter(|r| r.is_subscription())
                    .map(|r| (r.name.as_str(), r))
            })
            .collect()
    };

    let month_secs = 30.0 * 86_400.0;
    let window_fraction = (window_secs.max(0.0)) / month_secs;
    let accrued = |fee: f64| fee * window_fraction;

    // Total tokens (in + out + cache) a record consumed — the allocation weight
    // for a subscription fee (subscriptions carry no per-token rates).
    let record_tokens = |rec: &DashboardUsageRecord| -> f64 {
        (rec.input_tokens.max(0)
            + rec.output_tokens.max(0)
            + rec.cache_read_tokens.max(0)
            + rec.cache_write_tokens.max(0)) as f64
    };

    // Sum tokens per subscription rule (denominator for fee allocation).
    let sub_token_total = |records: &[DashboardUsageRecord]| -> HashMap<String, f64> {
        let mut m: HashMap<String, f64> = HashMap::new();
        for rec in records {
            if let Some(rule) = rule_for(rec)
                && rule.is_subscription()
            {
                *m.entry(rule.name.clone()).or_insert(0.0) += record_tokens(rec);
            }
        }
        m
    };

    // Per-record actual: PAYG = reference cost; subscription = accrued fee × token share.
    let actual_of = |rule: &crate::config::PricingRule,
                     rec: &DashboardUsageRecord,
                     reference: f64,
                     tok_totals: &HashMap<String, f64>|
     -> f64 {
        if rule.is_subscription() {
            let total = tok_totals.get(&rule.name).copied().unwrap_or(0.0);
            if total > 0.0 {
                accrued(rule.monthly_fee) * (record_tokens(rec) / total)
            } else {
                0.0
            }
        } else {
            reference
        }
    };

    let cur_tok_totals = sub_token_total(current);
    let mut by_member: HashMap<String, (f64, f64)> = HashMap::new();
    let mut by_model: HashMap<String, (f64, f64)> = HashMap::new();
    let mut actual_total = 0.0;
    let mut reference_total = 0.0;

    for rec in current {
        let Some(rule) = rule_for(rec) else { continue };
        let reference = reference_cost_of(rec, rule, unit);
        let actual = actual_of(rule, rec, reference, &cur_tok_totals);
        reference_total += reference;
        actual_total += actual;
        let m = by_member.entry(rec.team_id.clone()).or_insert((0.0, 0.0));
        m.0 += actual;
        m.1 += reference;
        let md = by_model.entry(rec.model.clone()).or_insert((0.0, 0.0));
        md.0 += actual;
        md.1 += reference;
    }

    // Subscription rows. A rule referenced by a channel but with no traffic still
    // accrues its fee — that's pure waste (idle_fee), and still counts as spent.
    let mut subscriptions: Vec<DashboardSubscriptionItem> = sub_rules
        .iter()
        .map(|(name, rule)| {
            let acc = accrued(rule.monthly_fee);
            let toks = cur_tok_totals.get(*name).copied().unwrap_or(0.0);
            let idle = if toks > 0.0 { 0.0 } else { acc };
            actual_total += idle;
            let quota = rule
                .included_quota_tokens
                .map(|q| q as f64 * window_fraction)
                .unwrap_or(0.0);
            DashboardSubscriptionItem {
                name: (*name).to_string(),
                monthly_fee: rule.monthly_fee,
                accrued_fee: acc,
                tokens_used: toks,
                quota_tokens: quota,
                utilization: if quota > 0.0 { toks / quota } else { 0.0 },
                idle_fee: idle,
            }
        })
        .collect();
    subscriptions.sort_by(|a, b| b.accrued_fee.total_cmp(&a.accrued_fee));

    // Previous-window actual (for the delta), same rules.
    let prev_tok_totals = sub_token_total(previous);
    let mut prev_actual = 0.0;
    for rec in previous {
        if let Some(rule) = rule_for(rec) {
            let reference = reference_cost_of(rec, rule, unit);
            prev_actual += actual_of(rule, rec, reference, &prev_tok_totals);
        }
    }
    for (name, rule) in &sub_rules {
        if prev_tok_totals.get(*name).copied().unwrap_or(0.0) <= 0.0 {
            prev_actual += accrued(rule.monthly_fee);
        }
    }

    let mut by_member: Vec<DashboardCostMemberItem> = by_member
        .into_iter()
        .map(|(id, (actual, reference))| {
            let group = group_by_member.get(id.as_str()).cloned().flatten();
            DashboardCostMemberItem {
                id,
                group,
                actual_cost: actual,
                reference_cost: reference,
            }
        })
        .collect();
    by_member.sort_by(|a, b| b.actual_cost.total_cmp(&a.actual_cost));
    by_member.truncate(50);

    let mut by_model: Vec<DashboardCostModelItem> = by_model
        .into_iter()
        .map(|(name, (actual, reference))| DashboardCostModelItem {
            name,
            actual_cost: actual,
            reference_cost: reference,
        })
        .collect();
    by_model.sort_by(|a, b| b.reference_cost.total_cmp(&a.reference_cost));
    by_model.truncate(50);

    DashboardCostSection {
        currency: pricing.currency.clone(),
        actual_cost: actual_total,
        reference_cost: reference_total,
        delta_actual_cost: percent_change(actual_total, prev_actual),
        effective_discount: if reference_total > 0.0 {
            1.0 - actual_total / reference_total
        } else {
            0.0
        },
        by_member,
        by_model,
        subscriptions,
    }
}

/// Reference cost of one rollup bucket under its channel's PAYG rule (0 for
/// subscriptions / unpriced). The rollup analogue of [`reference_cost_of`].
pub(super) fn rollup_reference_cost(
    row: &RollupRow,
    rule: &crate::config::PricingRule,
    unit: f64,
) -> f64 {
    if rule.is_subscription() {
        return 0.0;
    }
    match rule.price_for(&row.model) {
        Some(p) => {
            (p.input * row.input_tokens.max(0) as f64
                + p.output * row.output_tokens.max(0) as f64
                + p.cache_read_rate() * row.cache_read_tokens.max(0) as f64
                + p.cache_write_rate() * row.cache_write_tokens.max(0) as f64)
                / unit.max(1.0)
        }
        None => 0.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::test_fixtures::*;

    // ---- cost section ----

    #[allow(clippy::too_many_arguments)]
    fn cost_rec(
        id: i64,
        team: &str,
        channel: &str,
        model: &str,
        input: i64,
        output: i64,
        cache_read: i64,
        cache_write: i64,
    ) -> DashboardUsageRecord {
        DashboardUsageRecord {
            id,
            timestamp: "2026-03-12 12:00:00".to_string(),
            request_id: None,
            team_id: team.to_string(),
            router: "default".to_string(),
            matched_rule: None,
            final_channel: channel.to_string(),
            channel: channel.to_string(),
            model: model.to_string(),
            input_tokens: input,
            output_tokens: output,
            latency_ms: Some(50.0),
            fallback_triggered: false,
            status: "success".to_string(),
            status_code: Some(200),
            error_message: None,
            provider_trace_id: None,
            provider_error_body: None,
            client: None,
            user_agent: None,
            cache_read_tokens: cache_read,
            cache_write_tokens: cache_write,
            req_hash: None,
            session_key: None,
        }
    }

    #[test]
    fn reference_cost_uses_rule_rate_card() {
        let rule = payg_rule("std", 2.5, 10.0, Some(1.25));
        // 1M input @2.5 + 1M output @10 + 1M cache_read @1.25 = 13.75
        let rec = cost_rec(1, "a", "c", "any-model", 1_000_000, 1_000_000, 1_000_000, 0);
        assert!((reference_cost_of(&rec, &rule, 1_000_000.0) - 13.75).abs() < 1e-9);
    }

    #[test]
    fn reference_cost_prices_models_differently_within_one_rule() {
        // Same rule/channel, two model tiers priced differently (DeepSeek-style).
        let rule = payg_card(
            "deepseek",
            vec![
                price_row("*flash*", 0.14, 0.28, Some(0.0028)),
                price_row("*pro*", 0.435, 0.87, Some(0.003625)),
                price_row("*", 0.27, 1.1, None),
            ],
        );
        let flash = cost_rec(1, "a", "c", "deepseek-v4-flash", 1_000_000, 1_000_000, 0, 0);
        let pro = cost_rec(2, "a", "c", "deepseek-v4-pro", 1_000_000, 1_000_000, 0, 0);
        assert!((reference_cost_of(&flash, &rule, 1_000_000.0) - (0.14 + 0.28)).abs() < 1e-9);
        assert!((reference_cost_of(&pro, &rule, 1_000_000.0) - (0.435 + 0.87)).abs() < 1e-9);
        // unmatched model falls through to the `*` row
        let other = cost_rec(3, "a", "c", "deepseek-chat", 1_000_000, 0, 0, 0);
        assert!((reference_cost_of(&other, &rule, 1_000_000.0) - 0.27).abs() < 1e-9);
    }

    #[test]
    fn cost_section_payg_actual_equals_reference() {
        let pricing = pricing_of(vec![payg_rule("std", 2.5, 10.0, None)]);
        let channels = vec![priced_channel("openai", "std")];
        let recs = vec![
            cost_rec(1, "alice", "openai", "m", 1_000_000, 0, 0, 0), // $2.5
            cost_rec(2, "bob", "openai", "m", 0, 1_000_000, 0, 0),   // $10
        ];
        let c = build_cost_section(&recs, &[], &pricing, &channels, &[], 86_400.0, false);
        assert!((c.actual_cost - 12.5).abs() < 1e-9);
        assert!((c.reference_cost - 12.5).abs() < 1e-9);
        assert!(c.subscriptions.is_empty());
        let alice = c.by_member.iter().find(|m| m.id == "alice").unwrap();
        assert!((alice.actual_cost - 2.5).abs() < 1e-9);
    }

    #[test]
    fn cost_section_untracked_channel_is_excluded() {
        // No channel selects a rule → nothing is priced.
        let pricing = pricing_of(vec![payg_rule("std", 2.5, 10.0, None)]);
        let recs = vec![cost_rec(1, "alice", "openai", "m", 1_000_000, 0, 0, 0)];
        let c = build_cost_section(&recs, &[], &pricing, &[], &[], 86_400.0, false);
        assert_eq!(c.actual_cost, 0.0);
        assert_eq!(c.reference_cost, 0.0);
        assert!(c.by_member.is_empty());
    }

    #[test]
    fn cost_section_subscription_allocates_fee_by_token_share() {
        // Subscription: no per-token rates; fee split by token usage.
        let pricing = pricing_of(vec![sub_rule("claude-plan", 300.0, None)]);
        let channels = vec![priced_channel("sub-ch", "claude-plan")];
        // alice 2M tokens, bob 1M tokens -> 2:1 split
        let recs = vec![
            cost_rec(1, "alice", "sub-ch", "m", 2_000_000, 0, 0, 0),
            cost_rec(2, "bob", "sub-ch", "m", 1_000_000, 0, 0, 0),
        ];
        let month = 30.0 * 86_400.0; // full month => accrued == monthly_fee
        let c = build_cost_section(&recs, &[], &pricing, &channels, &[], month, false);
        assert!(
            (c.actual_cost - 300.0).abs() < 1e-6,
            "actual {}",
            c.actual_cost
        );
        assert_eq!(c.reference_cost, 0.0); // subscriptions have no reference $
        let sub = &c.subscriptions[0];
        assert_eq!(sub.name, "claude-plan");
        assert!((sub.accrued_fee - 300.0).abs() < 1e-6);
        assert!((sub.tokens_used - 3_000_000.0).abs() < 1e-6);
        assert_eq!(sub.quota_tokens, 0.0); // no quota configured
        assert_eq!(sub.idle_fee, 0.0);
        let alice = c.by_member.iter().find(|m| m.id == "alice").unwrap();
        let bob = c.by_member.iter().find(|m| m.id == "bob").unwrap();
        assert!(
            (alice.actual_cost - 200.0).abs() < 1e-6,
            "alice {}",
            alice.actual_cost
        );
        assert!((bob.actual_cost - 100.0).abs() < 1e-6);
    }

    #[test]
    fn cost_section_subscription_utilization_is_quota_based() {
        // 10M/month quota; 5M used over a full month => 50% utilization.
        let pricing = pricing_of(vec![sub_rule("plan", 300.0, Some(10_000_000))]);
        let channels = vec![priced_channel("ch", "plan")];
        let recs = vec![cost_rec(1, "a", "ch", "m", 5_000_000, 0, 0, 0)];
        let c = build_cost_section(&recs, &[], &pricing, &channels, &[], 30.0 * 86_400.0, false);
        let sub = &c.subscriptions[0];
        assert!((sub.tokens_used - 5_000_000.0).abs() < 1e-6);
        assert!((sub.quota_tokens - 10_000_000.0).abs() < 1e-3);
        assert!(
            (sub.utilization - 0.5).abs() < 1e-6,
            "util {}",
            sub.utilization
        );
    }

    #[test]
    fn cost_section_idle_subscription_is_pure_waste() {
        let pricing = pricing_of(vec![sub_rule("idle-plan", 300.0, None)]);
        let channels = vec![priced_channel("idle-ch", "idle-plan")];
        let c = build_cost_section(&[], &[], &pricing, &channels, &[], 30.0 * 86_400.0, false);
        assert!((c.actual_cost - 300.0).abs() < 1e-6);
        assert_eq!(c.reference_cost, 0.0);
        let sub = &c.subscriptions[0];
        assert!((sub.idle_fee - 300.0).abs() < 1e-6);
        assert_eq!(sub.tokens_used, 0.0);
        assert_eq!(sub.utilization, 0.0);
    }

    #[test]
    fn cost_section_filtered_excludes_untrafficked_subscriptions() {
        // Idle subscription (channel referenced, no records in window).
        let pricing = pricing_of(vec![sub_rule("idle-plan", 300.0, None)]);
        let channels = vec![priced_channel("idle-ch", "idle-plan")];
        // Unfiltered: fee counts as idle waste.
        let full = build_cost_section(&[], &[], &pricing, &channels, &[], 30.0 * 86_400.0, false);
        assert!((full.actual_cost - 300.0).abs() < 1e-6);
        assert_eq!(full.subscriptions.len(), 1);
        // Filtered: no traffic for this rule in scope ⇒ its fee must not leak in.
        let scoped = build_cost_section(&[], &[], &pricing, &channels, &[], 30.0 * 86_400.0, true);
        assert_eq!(scoped.actual_cost, 0.0);
        assert!(scoped.subscriptions.is_empty());
    }
}
