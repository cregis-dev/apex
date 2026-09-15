use crate::config::Channel;
use crate::database::{RollupRow, UsageRecord as DashboardUsageRecord};

use super::cost::{reference_cost_of, rollup_reference_cost};
use super::*;

// Baseline window for per-member z-scores, and the minimum number of populated
// hourly buckets before a baseline is trusted enough to score against.
pub(super) const BEHAVIOR_BASELINE_DAYS: i64 = 7;
const BEHAVIOR_MIN_BASELINE_BUCKETS: usize = 12;
// Working hours are [08:00, 20:00) local; activity outside is "night" — a proxy
// for automation (a 24/7 flat profile reads as a machine, not a person).
const WORK_HOUR_START: u32 = 8;
const WORK_HOUR_END: u32 = 20;

pub(super) fn is_night_hour(hour: u32) -> bool {
    !(WORK_HOUR_START..WORK_HOUR_END).contains(&hour)
}

/// Population mean and standard deviation, or `None` for an empty slice.
pub(super) fn behavior_mean_std(values: &[f64]) -> Option<(f64, f64)> {
    if values.is_empty() {
        return None;
    }
    let n = values.len() as f64;
    let mean = values.iter().sum::<f64>() / n;
    let variance = values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / n;
    Some((mean, variance.sqrt()))
}

pub(super) fn behavior_flag(
    signal: &str,
    value: f64,
    threshold: f64,
    severity: &str,
    action: &str,
    detail: String,
) -> DashboardBehaviorFlag {
    DashboardBehaviorFlag {
        signal: signal.to_string(),
        severity: severity.to_string(),
        value,
        threshold,
        detail,
        suggested_action: action.to_string(),
    }
}

/// Critical once the value is halfway from the threshold to the 1.0 ceiling.
pub(super) fn behavior_ratio_severity(value: f64, threshold: f64) -> &'static str {
    if value >= (threshold + 1.0) / 2.0 {
        "critical"
    } else {
        "warning"
    }
}

/// Critical at twice the z threshold.
pub(super) fn behavior_z_severity(z: f64, threshold: f64) -> &'static str {
    if z >= threshold * 2.0 {
        "critical"
    } else {
        "warning"
    }
}

pub(super) fn behavior_action_rank(action: &str) -> u8 {
    match action {
        "disable" => 2,
        "rate_limit" => 1,
        _ => 0,
    }
}

pub(super) fn behavior_severity_rank(severity: &str) -> u8 {
    match severity {
        "critical" => 1,
        _ => 0,
    }
}

/// Escalate a member's flags into one suggested action. Two or more critical
/// flags whose *own* suggestion is actionable (not `observe`) ⇒ disable;
/// otherwise the strongest per-flag suggestion. Gating on actionable-criticals
/// (rather than raw critical count) keeps benign automation — e.g. a nightly
/// cron that trips critical `off_hours` + `output_zero`, both observe-only —
/// from being escalated to a disable recommendation.
pub(super) fn behavior_escalate(flags: &[DashboardBehaviorFlag]) -> String {
    let actionable_criticals = flags
        .iter()
        .filter(|f| f.severity == "critical" && f.suggested_action != "observe")
        .count();
    if actionable_criticals >= 2 {
        return "disable".to_string();
    }
    let top = flags
        .iter()
        .map(|f| behavior_action_rank(&f.suggested_action))
        .max()
        .unwrap_or(0);
    match top {
        2 => "disable",
        1 => "rate_limit",
        _ => "observe",
    }
    .to_string()
}

/// Read-time, stateless abuse/waste detection over the analytics window.
///
/// Two tiers, both computed live (no flag storage): rule thresholds on
/// window metrics (repeat rate, error storm, zero output, off-hours), plus a
/// baseline overlay that z-scores the window's request/spend rate against the
/// member's own rolling `usage_rollup` history. Suggestions are advisory —
/// operators confirm before any `rate_limit`/`disable` is applied.
#[allow(clippy::too_many_arguments)]
pub(super) fn build_behavior_section(
    current: &[DashboardUsageRecord],
    baseline: &[RollupRow],
    thresholds: &crate::config::Thresholds,
    teams: &[crate::config::Team],
    pricing: Option<&crate::config::Pricing>,
    channels: &[Channel],
    window: &DashboardWindow,
) -> DashboardBehaviorSection {
    use chrono::Timelike;
    use std::collections::{HashMap, HashSet};

    let window_secs = (window.current_end - window.current_start)
        .num_seconds()
        .max(0) as f64;
    // Rate is per-hour; floor the divisor so a sub-minute window can't explode it.
    let window_hours = (window_secs / 3600.0).max(1.0 / 60.0);

    let group_by_member: HashMap<&str, Option<String>> = teams
        .iter()
        .map(|t| (t.id.as_str(), t.group.clone()))
        .collect();

    // Pricing maps for the spend signals. Empty `rules` ⇒ spend signals disabled.
    let pricing_enabled = pricing.is_some();
    let unit = pricing.map(|p| p.unit).unwrap_or(1.0);
    let channel_rule: HashMap<&str, &str> = channels
        .iter()
        .filter_map(|ch| ch.pricing.as_deref().map(|r| (ch.name.as_str(), r)))
        .collect();
    let rules: HashMap<&str, &crate::config::PricingRule> = pricing
        .map(|p| p.rules.iter().map(|r| (r.name.as_str(), r)).collect())
        .unwrap_or_default();
    let rule_for_channel = |channel: &str| -> Option<&crate::config::PricingRule> {
        channel_rule
            .get(channel)
            .and_then(|name| rules.get(name).copied())
    };

    // Per-member window aggregation.
    #[derive(Default)]
    struct Acc {
        requests: i64,
        errors: i64,
        successes: i64,
        zero_output: i64,
        night: i64,
        hashed: i64,
        ref_cost: f64,
        hashes: HashSet<String>,
    }
    let mut members: HashMap<&str, Acc> = HashMap::new();
    for rec in current {
        let acc = members.entry(rec.team_id.as_str()).or_default();
        acc.requests += 1;
        if usage_record_is_error(rec) {
            acc.errors += 1;
        } else {
            acc.successes += 1;
            if rec.output_tokens <= 0 && rec.input_tokens > 0 {
                acc.zero_output += 1;
            }
        }
        if let Some(ts) = parse_dashboard_timestamp(&rec.timestamp)
            && is_night_hour(ts.hour())
        {
            acc.night += 1;
        }
        if let Some(hash) = &rec.req_hash {
            acc.hashed += 1;
            acc.hashes.insert(hash.clone());
        }
        if pricing_enabled && let Some(rule) = rule_for_channel(&rec.channel) {
            acc.ref_cost += reference_cost_of(rec, rule, unit);
        }
    }

    // Per-member baseline series from rollup: member -> bucket_start -> value.
    let mut base_rate: HashMap<&str, HashMap<&str, f64>> = HashMap::new();
    let mut base_spend: HashMap<&str, HashMap<&str, f64>> = HashMap::new();
    for row in baseline {
        *base_rate
            .entry(row.team_id.as_str())
            .or_default()
            .entry(row.bucket_start.as_str())
            .or_insert(0.0) += row.requests as f64;
        if pricing_enabled && let Some(rule) = rule_for_channel(&row.channel) {
            *base_spend
                .entry(row.team_id.as_str())
                .or_default()
                .entry(row.bucket_start.as_str())
                .or_insert(0.0) += rollup_reference_cost(row, rule, unit);
        }
    }
    // z-score of `current_rate` against an hourly baseline series, or None when
    // the baseline is too sparse or has no spread.
    let z_of = |series: Option<&HashMap<&str, f64>>, current_rate: f64| -> Option<f64> {
        let series = series?;
        if series.len() < BEHAVIOR_MIN_BASELINE_BUCKETS {
            return None;
        }
        let values: Vec<f64> = series.values().copied().collect();
        let (mean, std) = behavior_mean_std(&values)?;
        if std <= f64::EPSILON {
            return None;
        }
        Some((current_rate - mean) / std)
    };

    let min_samples = thresholds.min_samples as i64;
    let mut evaluated = 0usize;
    let mut out: Vec<DashboardBehaviorMember> = Vec::new();

    for (id, acc) in &members {
        // Too few samples to judge fairly — skip (suppresses false positives).
        if acc.requests < min_samples {
            continue;
        }
        evaluated += 1;
        let requests = acc.requests as f64;

        let repeats = acc.hashed - acc.hashes.len() as i64;
        let repeat_rate = if acc.hashed >= 2 {
            1.0 - (acc.hashes.len() as f64 / acc.hashed as f64)
        } else {
            0.0
        };
        let error_ratio = acc.errors as f64 / requests;
        let output_zero_ratio = if acc.successes > 0 {
            acc.zero_output as f64 / acc.successes as f64
        } else {
            0.0
        };
        let night_ratio = acc.night as f64 / requests;

        let current_rate = requests / window_hours;
        let rate_z = z_of(base_rate.get(*id), current_rate);
        let (spend_z, reference_cost) = if pricing_enabled {
            let spend_rate = acc.ref_cost / window_hours;
            (z_of(base_spend.get(*id), spend_rate), Some(acc.ref_cost))
        } else {
            (None, None)
        };

        let mut flags: Vec<DashboardBehaviorFlag> = Vec::new();
        // Repeat rate — only with enough fingerprinted samples to be meaningful.
        if acc.hashed >= min_samples && repeat_rate >= thresholds.repeat_rate {
            flags.push(behavior_flag(
                "repeat_rate",
                repeat_rate,
                thresholds.repeat_rate,
                behavior_ratio_severity(repeat_rate, thresholds.repeat_rate),
                "rate_limit",
                format!(
                    "{repeats} of {} fingerprinted requests are repeats",
                    acc.hashed
                ),
            ));
        }
        if error_ratio >= thresholds.error_ratio {
            flags.push(behavior_flag(
                "error_storm",
                error_ratio,
                thresholds.error_ratio,
                behavior_ratio_severity(error_ratio, thresholds.error_ratio),
                "rate_limit",
                format!("{}/{} requests errored", acc.errors, acc.requests),
            ));
        }
        if output_zero_ratio >= thresholds.output_zero_ratio {
            flags.push(behavior_flag(
                "output_zero",
                output_zero_ratio,
                thresholds.output_zero_ratio,
                behavior_ratio_severity(output_zero_ratio, thresholds.output_zero_ratio),
                "observe",
                format!(
                    "{}/{} successful requests produced ~zero output",
                    acc.zero_output, acc.successes
                ),
            ));
        }
        if night_ratio >= thresholds.night_ratio {
            flags.push(behavior_flag(
                "off_hours",
                night_ratio,
                thresholds.night_ratio,
                behavior_ratio_severity(night_ratio, thresholds.night_ratio),
                "observe",
                format!(
                    "{:.0}% of requests fell outside working hours",
                    night_ratio * 100.0
                ),
            ));
        }
        if let Some(z) = rate_z
            && z >= thresholds.rate_spike_z
        {
            flags.push(behavior_flag(
                "rate_spike",
                z,
                thresholds.rate_spike_z,
                behavior_z_severity(z, thresholds.rate_spike_z),
                "rate_limit",
                format!("request rate {z:.1}σ above the member's baseline"),
            ));
        }
        if let Some(z) = spend_z
            && z >= thresholds.spend_spike_z
        {
            flags.push(behavior_flag(
                "spend_spike",
                z,
                thresholds.spend_spike_z,
                behavior_z_severity(z, thresholds.spend_spike_z),
                "rate_limit",
                format!("spend {z:.1}σ above the member's baseline"),
            ));
        }

        if flags.is_empty() {
            continue;
        }
        let severity = if flags.iter().any(|f| f.severity == "critical") {
            "critical"
        } else {
            "warning"
        };
        let suggested_action = behavior_escalate(&flags);
        out.push(DashboardBehaviorMember {
            id: (*id).to_string(),
            group: group_by_member.get(*id).cloned().flatten(),
            severity: severity.to_string(),
            suggested_action,
            profile: DashboardBehaviorProfile {
                requests: acc.requests,
                repeat_rate,
                error_ratio,
                output_zero_ratio,
                night_ratio,
                rate_z,
                spend_z,
                reference_cost,
            },
            flags,
        });
    }

    // Most severe first, then busiest.
    out.sort_by(|a, b| {
        behavior_severity_rank(&b.severity)
            .cmp(&behavior_severity_rank(&a.severity))
            .then(b.profile.requests.cmp(&a.profile.requests))
    });
    let flagged = out.len();

    DashboardBehaviorSection {
        window_secs,
        evaluated,
        flagged,
        members: out,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- Behavior detection helpers + tests ---

    /// A usage record with full control over the fields behavior detection reads.
    #[allow(clippy::too_many_arguments)]
    fn bhv_rec(
        id: i64,
        team: &str,
        timestamp: &str,
        status: &str,
        input: i64,
        output: i64,
        hash: Option<&str>,
    ) -> DashboardUsageRecord {
        DashboardUsageRecord {
            id,
            timestamp: timestamp.to_string(),
            request_id: None,
            team_id: team.to_string(),
            router: "default".to_string(),
            matched_rule: None,
            final_channel: "openai".to_string(),
            channel: "openai".to_string(),
            model: "gpt-4o".to_string(),
            input_tokens: input,
            output_tokens: output,
            latency_ms: Some(50.0),
            fallback_triggered: false,
            status: status.to_string(),
            status_code: Some(200),
            error_message: None,
            provider_trace_id: None,
            provider_error_body: None,
            client: None,
            user_agent: None,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
            req_hash: hash.map(|h| h.to_string()),
            session_key: None,
        }
    }

    /// A window starting at `start` (local `%Y-%m-%d %H:%M:%S`) spanning `hours`.
    fn bhv_window(start: &str, hours: i64) -> DashboardWindow {
        let s = chrono::NaiveDateTime::parse_from_str(start, "%Y-%m-%d %H:%M:%S").unwrap();
        let span = chrono::Duration::hours(hours);
        DashboardWindow {
            range: "custom".to_string(),
            bucket: DashboardBucket::Hour,
            current_start: s,
            current_end: s + span,
            previous_start: s - span,
            previous_end: s,
        }
    }

    #[test]
    fn behavior_mean_std_matches_known_dataset() {
        assert_eq!(behavior_mean_std(&[]), None);
        // Classic set with population mean 5, population std 2.
        let (mean, std) = behavior_mean_std(&[2.0, 4.0, 4.0, 4.0, 5.0, 5.0, 7.0, 9.0]).unwrap();
        assert!((mean - 5.0).abs() < 1e-9);
        assert!((std - 2.0).abs() < 1e-9);
    }

    #[test]
    fn behavior_flags_high_repeat_rate() {
        let t = crate::config::Thresholds::default(); // repeat_rate 0.5, min_samples 50
        let recs: Vec<_> = (0..60)
            .map(|i| {
                bhv_rec(
                    i,
                    "alice",
                    "2026-03-10 12:00:00",
                    "success",
                    10,
                    20,
                    Some("same"),
                )
            })
            .collect();
        let section = build_behavior_section(
            &recs,
            &[],
            &t,
            &[],
            None,
            &[],
            &bhv_window("2026-03-10 12:00:00", 1),
        );
        assert_eq!(section.evaluated, 1);
        assert_eq!(section.flagged, 1);
        let member = &section.members[0];
        assert_eq!(member.id, "alice");
        let flag = member
            .flags
            .iter()
            .find(|f| f.signal == "repeat_rate")
            .unwrap();
        assert_eq!(flag.severity, "critical"); // ~0.983 >= (0.5+1)/2
        assert_eq!(flag.suggested_action, "rate_limit");
    }

    #[test]
    fn behavior_skips_members_below_min_samples() {
        let t = crate::config::Thresholds::default();
        let recs: Vec<_> = (0..10)
            .map(|i| {
                bhv_rec(
                    i,
                    "alice",
                    "2026-03-10 12:00:00",
                    "success",
                    10,
                    20,
                    Some("same"),
                )
            })
            .collect();
        let section = build_behavior_section(
            &recs,
            &[],
            &t,
            &[],
            None,
            &[],
            &bhv_window("2026-03-10 12:00:00", 1),
        );
        assert_eq!(section.evaluated, 0);
        assert!(section.members.is_empty());
    }

    #[test]
    fn behavior_flags_error_storm() {
        let t = crate::config::Thresholds::default(); // error_ratio 0.3
        let recs: Vec<_> = (0..60)
            .map(|i| {
                let status = if i < 30 { "error" } else { "success" };
                bhv_rec(i, "alice", "2026-03-10 12:00:00", status, 10, 20, None)
            })
            .collect();
        let section = build_behavior_section(
            &recs,
            &[],
            &t,
            &[],
            None,
            &[],
            &bhv_window("2026-03-10 12:00:00", 1),
        );
        let member = &section.members[0];
        let flag = member
            .flags
            .iter()
            .find(|f| f.signal == "error_storm")
            .unwrap();
        assert!((flag.value - 0.5).abs() < 1e-9);
        assert_eq!(flag.severity, "warning"); // 0.5 < (0.3+1)/2
        assert_eq!(member.suggested_action, "rate_limit");
    }

    #[test]
    fn behavior_flags_zero_output() {
        let t = crate::config::Thresholds::default(); // output_zero_ratio 0.4
        let recs: Vec<_> = (0..60)
            .map(|i| bhv_rec(i, "alice", "2026-03-10 12:00:00", "success", 10, 0, None))
            .collect();
        let section = build_behavior_section(
            &recs,
            &[],
            &t,
            &[],
            None,
            &[],
            &bhv_window("2026-03-10 12:00:00", 1),
        );
        let member = &section.members[0];
        assert!(member.flags.iter().any(|f| f.signal == "output_zero"));
        assert_eq!(member.suggested_action, "observe"); // output_zero suggests observe only
    }

    #[test]
    fn behavior_flags_off_hours() {
        let t = crate::config::Thresholds::default(); // night_ratio 0.6
        let recs: Vec<_> = (0..60)
            .map(|i| bhv_rec(i, "alice", "2026-03-10 03:00:00", "success", 10, 20, None))
            .collect();
        let section = build_behavior_section(
            &recs,
            &[],
            &t,
            &[],
            None,
            &[],
            &bhv_window("2026-03-10 00:00:00", 24),
        );
        let member = &section.members[0];
        assert!(member.flags.iter().any(|f| f.signal == "off_hours"));
    }

    #[test]
    fn behavior_flags_rate_spike_against_baseline() {
        let t = crate::config::Thresholds::default(); // rate_spike_z 3.0
        // 24 hourly baseline buckets, mostly ~1 req/hr with a little spread.
        let baseline: Vec<RollupRow> = (0..24)
            .map(|h| RollupRow {
                bucket_start: format!("2026-03-09 {h:02}:00:00"),
                team_id: "alice".to_string(),
                model: "gpt-4o".to_string(),
                channel: "openai".to_string(),
                requests: if h % 6 == 0 { 2 } else { 1 },
                error_requests: 0,
                input_tokens: 10,
                output_tokens: 20,
                cache_read_tokens: 0,
                cache_write_tokens: 0,
                zero_output_reqs: 0,
                latency_sum_ms: 0.0,
                latency_count: 0,
            })
            .collect();
        // 80 requests in a 1h window ⇒ ~80/hr, far above the ~1.2/hr baseline.
        let recs: Vec<_> = (0..80)
            .map(|i| bhv_rec(i, "alice", "2026-03-10 12:00:00", "success", 10, 20, None))
            .collect();
        let section = build_behavior_section(
            &recs,
            &baseline,
            &t,
            &[],
            None,
            &[],
            &bhv_window("2026-03-10 12:00:00", 1),
        );
        let member = &section.members[0];
        let flag = member
            .flags
            .iter()
            .find(|f| f.signal == "rate_spike")
            .unwrap();
        assert!(flag.value > 3.0);
        assert_eq!(flag.severity, "critical");
    }

    #[test]
    fn behavior_baseline_excludes_the_windows_own_hour_bucket() {
        // The window starts mid-hour (12:37); a huge bucket sits at the window's
        // own start-hour (12:00). If the half-open baseline range weren't floored
        // to the hour, that bucket would leak into the baseline, inflate its
        // mean/variance, and suppress the rate_spike z-score. With flooring it is
        // excluded, so the spike still fires.
        let t = crate::config::Thresholds::default();
        let mut baseline: Vec<RollupRow> = (0..15)
            .map(|h| RollupRow {
                bucket_start: format!("2026-03-09 {h:02}:00:00"),
                team_id: "alice".to_string(),
                model: "gpt-4o".to_string(),
                channel: "openai".to_string(),
                requests: if h % 5 == 0 { 2 } else { 1 }, // spread ⇒ non-zero variance
                error_requests: 0,
                input_tokens: 10,
                output_tokens: 20,
                cache_read_tokens: 0,
                cache_write_tokens: 0,
                zero_output_reqs: 0,
                latency_sum_ms: 0.0,
                latency_count: 0,
            })
            .collect();
        // Poison bucket AT the window's start hour — must be excluded.
        baseline.push(RollupRow {
            bucket_start: "2026-03-10 12:00:00".to_string(),
            team_id: "alice".to_string(),
            model: "gpt-4o".to_string(),
            channel: "openai".to_string(),
            requests: 5000,
            error_requests: 0,
            input_tokens: 10,
            output_tokens: 20,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
            zero_output_reqs: 0,
            latency_sum_ms: 0.0,
            latency_count: 0,
        });
        let recs: Vec<_> = (0..80)
            .map(|i| bhv_rec(i, "alice", "2026-03-10 12:40:00", "success", 10, 20, None))
            .collect();
        // The handler floors both bounds to the hour before calling
        // get_rollup_between; emulate that here (start = window - 7d, floored).
        let start = "2026-03-03 12:00:00";
        let end = "2026-03-10 12:00:00";
        let in_range: Vec<RollupRow> = baseline
            .into_iter()
            .filter(|r| r.bucket_start.as_str() >= start && r.bucket_start.as_str() < end)
            .collect();
        assert_eq!(
            in_range.len(),
            15,
            "the 12:00 poison bucket must be excluded"
        );
        let section = build_behavior_section(
            &recs,
            &in_range,
            &t,
            &[],
            None,
            &[],
            &bhv_window("2026-03-10 12:40:00", 1),
        );
        assert!(
            section.members[0]
                .flags
                .iter()
                .any(|f| f.signal == "rate_spike"),
            "rate_spike must fire once the window's own hour is excluded from the baseline"
        );
    }

    #[test]
    fn behavior_clean_member_produces_no_flags() {
        let t = crate::config::Thresholds::default();
        // Distinct hashes, real output, daytime, no errors, no baseline.
        let recs: Vec<_> = (0..60)
            .map(|i| {
                let h = format!("h{i}");
                bhv_rec(
                    i,
                    "alice",
                    "2026-03-10 12:00:00",
                    "success",
                    10,
                    20,
                    Some(&h),
                )
            })
            .collect();
        let section = build_behavior_section(
            &recs,
            &[],
            &t,
            &[],
            None,
            &[],
            &bhv_window("2026-03-10 12:00:00", 1),
        );
        assert_eq!(section.evaluated, 1);
        assert_eq!(section.flagged, 0);
        assert!(section.members.is_empty());
    }

    #[test]
    fn behavior_section_serializes_to_the_wire_shape_the_cp_expects() {
        // Guards the Rust → TS contract (cp/src/lib/types.ts BehaviorSection).
        let t = crate::config::Thresholds::default();
        let recs: Vec<_> = (0..60)
            .map(|i| {
                bhv_rec(
                    i,
                    "alice",
                    "2026-03-10 12:00:00",
                    "success",
                    10,
                    20,
                    Some("same"),
                )
            })
            .collect();
        let section = build_behavior_section(
            &recs,
            &[],
            &t,
            &[],
            None,
            &[],
            &bhv_window("2026-03-10 12:00:00", 1),
        );
        let v = serde_json::to_value(&section).unwrap();
        for key in ["window_secs", "evaluated", "flagged", "members"] {
            assert!(v.get(key).is_some(), "section missing {key}");
        }
        let member = &v["members"][0];
        for key in [
            "id",
            "group",
            "severity",
            "suggested_action",
            "profile",
            "flags",
        ] {
            assert!(member.get(key).is_some(), "member missing {key}");
        }
        let profile = &member["profile"];
        for key in [
            "requests",
            "repeat_rate",
            "error_ratio",
            "output_zero_ratio",
            "night_ratio",
        ] {
            assert!(profile.get(key).is_some(), "profile missing {key}");
        }
        // Optional z-scores are omitted (not null) when absent — matches `foo?: number`.
        assert!(profile.get("rate_z").is_none());
        let flag = &member["flags"][0];
        for key in [
            "signal",
            "severity",
            "value",
            "threshold",
            "detail",
            "suggested_action",
        ] {
            assert!(flag.get(key).is_some(), "flag missing {key}");
        }
    }

    #[test]
    fn behavior_escalates_to_disable_on_two_actionable_criticals() {
        let t = crate::config::Thresholds::default();
        // 100 identical-fingerprint requests, 70 errors ⇒ repeat_rate ~0.99
        // (critical, rate_limit) + error_ratio 0.70 (critical, rate_limit).
        let recs: Vec<_> = (0..100)
            .map(|i| {
                let status = if i < 70 { "error" } else { "success" };
                bhv_rec(
                    i,
                    "alice",
                    "2026-03-10 12:00:00",
                    status,
                    10,
                    20,
                    Some("same"),
                )
            })
            .collect();
        let section = build_behavior_section(
            &recs,
            &[],
            &t,
            &[],
            None,
            &[],
            &bhv_window("2026-03-10 12:00:00", 1),
        );
        let member = &section.members[0];
        assert_eq!(member.severity, "critical");
        assert_eq!(member.suggested_action, "disable");
    }

    #[test]
    fn behavior_does_not_disable_on_observe_only_criticals() {
        let t = crate::config::Thresholds::default();
        // Nightly cron: 60 zero-output successes at 03:00, distinct fingerprints,
        // no errors ⇒ off_hours (night 1.0) + output_zero (1.0), both critical but
        // both observe-only. Must NOT escalate to disable.
        let recs: Vec<_> = (0..60)
            .map(|i| {
                let h = format!("h{i}");
                bhv_rec(i, "cron", "2026-03-10 03:00:00", "success", 10, 0, Some(&h))
            })
            .collect();
        let section = build_behavior_section(
            &recs,
            &[],
            &t,
            &[],
            None,
            &[],
            &bhv_window("2026-03-10 00:00:00", 24),
        );
        let member = &section.members[0];
        let signals: Vec<_> = member.flags.iter().map(|f| f.signal.as_str()).collect();
        assert!(signals.contains(&"off_hours"));
        assert!(signals.contains(&"output_zero"));
        assert_eq!(member.severity, "critical"); // the flags are critical…
        assert_eq!(member.suggested_action, "observe"); // …but not actionable
    }
}
