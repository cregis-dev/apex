//! Pricing-config fixtures shared by the dashboard cost tests and the
//! `validate_pricing` test that still lives beside the admin pricing endpoint.
//! Kept in one place so neither side has to duplicate them.

use crate::config::Channel;

pub(crate) fn price_row(
    match_pattern: &str,
    input: f64,
    output: f64,
    cache_read: Option<f64>,
) -> crate::config::ModelPrice {
    crate::config::ModelPrice {
        match_pattern: match_pattern.to_string(),
        input,
        output,
        cache_read,
        cache_write: None,
    }
}

/// A PAYG rule with a single `*` rate-card row (the flat case).
pub(crate) fn payg_rule(
    name: &str,
    input: f64,
    output: f64,
    cache_read: Option<f64>,
) -> crate::config::PricingRule {
    crate::config::PricingRule {
        name: name.to_string(),
        kind: "payg".to_string(),
        prices: vec![price_row("*", input, output, cache_read)],
        monthly_fee: 0.0,
        billing_day: 1,
        included_quota_tokens: None,
    }
}

pub(crate) fn payg_card(
    name: &str,
    rows: Vec<crate::config::ModelPrice>,
) -> crate::config::PricingRule {
    crate::config::PricingRule {
        name: name.to_string(),
        kind: "payg".to_string(),
        prices: rows,
        monthly_fee: 0.0,
        billing_day: 1,
        included_quota_tokens: None,
    }
}

pub(crate) fn sub_rule(
    name: &str,
    monthly_fee: f64,
    quota: Option<u64>,
) -> crate::config::PricingRule {
    crate::config::PricingRule {
        name: name.to_string(),
        kind: "subscription".to_string(),
        prices: vec![],
        monthly_fee,
        billing_day: 1,
        included_quota_tokens: quota,
    }
}

pub(crate) fn pricing_of(rules: Vec<crate::config::PricingRule>) -> crate::config::Pricing {
    crate::config::Pricing {
        currency: "USD".to_string(),
        unit: 1_000_000.0,
        rules,
    }
}

/// A channel named `name` that bills under pricing rule `rule`.
pub(crate) fn priced_channel(name: &str, rule: &str) -> Channel {
    Channel {
        name: name.to_string(),
        provider_type: crate::config::ProviderType::Openai,
        base_url: "x".to_string(),
        api_key: "x".to_string(),
        anthropic_base_url: None,
        headers: None,
        model_map: None,
        timeouts: None,
        pricing: Some(rule.to_string()),
    }
}
