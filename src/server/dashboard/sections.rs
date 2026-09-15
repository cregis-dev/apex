use chrono::{Duration as ChronoDuration, Local, NaiveDateTime};
use std::collections::{BTreeMap, HashMap};

use crate::database::{UsageAggregate, UsageRecord as DashboardUsageRecord, UsageRecordQuery};

use super::*;

pub(super) fn dashboard_window(range: Option<&str>) -> DashboardWindow {
    let now = Local::now().naive_local();
    let (range_key, bucket, duration) = match range.unwrap_or("24h") {
        "1h" => ("1h", DashboardBucket::Hour, ChronoDuration::hours(1)),
        "7d" => ("7d", DashboardBucket::Day, ChronoDuration::days(7)),
        "30d" => ("30d", DashboardBucket::Day, ChronoDuration::days(30)),
        _ => ("24h", DashboardBucket::Hour, ChronoDuration::hours(24)),
    };
    let current_start = now - duration;
    let previous_end = current_start - ChronoDuration::seconds(1);
    let previous_start = previous_end - duration + ChronoDuration::seconds(1);

    DashboardWindow {
        range: range_key.to_string(),
        bucket,
        current_start,
        current_end: now,
        previous_start,
        previous_end,
    }
}

pub(super) fn format_dashboard_timestamp(value: NaiveDateTime) -> String {
    value.format("%Y-%m-%d %H:%M:%S").to_string()
}

pub(super) fn parse_dashboard_timestamp(value: &str) -> Option<NaiveDateTime> {
    NaiveDateTime::parse_from_str(value, "%Y-%m-%d %H:%M:%S")
        .or_else(|_| NaiveDateTime::parse_from_str(value, "%Y-%m-%dT%H:%M:%S"))
        .ok()
}

pub(super) fn normalize_query_filter(
    params: &HashMap<String, String>,
    key: &str,
) -> Option<String> {
    params
        .get(key)
        .map(|value| value.trim())
        .filter(|value| !value.is_empty() && *value != "all")
        .map(str::to_string)
}

pub(super) fn build_dashboard_usage_query(
    params: &HashMap<String, String>,
    start: NaiveDateTime,
    end: NaiveDateTime,
) -> UsageRecordQuery {
    UsageRecordQuery {
        team_id: normalize_query_filter(params, "team_id"),
        router: normalize_query_filter(params, "router"),
        channel: normalize_query_filter(params, "channel"),
        model: normalize_query_filter(params, "model"),
        status: normalize_query_filter(params, "status"),
        client: normalize_query_filter(params, "client"),
        start_time: Some(format_dashboard_timestamp(start)),
        end_time: Some(format_dashboard_timestamp(end)),
    }
}

pub(super) fn usage_record_total_tokens(record: &DashboardUsageRecord) -> i64 {
    record.input_tokens.max(0) + record.output_tokens.max(0)
}

pub(super) fn usage_record_is_error(record: &DashboardUsageRecord) -> bool {
    matches!(record.status.as_str(), "error" | "fallback_error")
}

pub(super) fn percent_change(current: f64, previous: f64) -> f64 {
    if previous.abs() < f64::EPSILON {
        return if current.abs() < f64::EPSILON {
            0.0
        } else {
            100.0
        };
    }

    ((current - previous) / previous) * 100.0
}

pub(super) fn percentile(values: &mut [f64], percentile: f64) -> f64 {
    if values.is_empty() {
        return 0.0;
    }

    values.sort_by(f64::total_cmp);
    let rank = ((values.len() - 1) as f64 * percentile.clamp(0.0, 1.0)).floor() as usize;
    values.get(rank).copied().unwrap_or(0.0)
}

pub(super) fn build_overview(
    current_records: &[DashboardUsageRecord],
    previous: &UsageAggregate,
) -> DashboardOverview {
    let total_requests = current_records.len() as i64;
    let input_tokens = current_records
        .iter()
        .map(|record| record.input_tokens.max(0))
        .sum();
    let output_tokens = current_records
        .iter()
        .map(|record| record.output_tokens.max(0))
        .sum();
    let total_tokens = input_tokens + output_tokens;
    let current_errors = current_records
        .iter()
        .filter(|record| usage_record_is_error(record))
        .count();
    let success_rate = if total_requests > 0 {
        ((total_requests - current_errors as i64) as f64 / total_requests as f64) * 100.0
    } else {
        0.0
    };
    let avg_latency_ms = {
        let latencies = current_records
            .iter()
            .filter_map(|record| record.latency_ms)
            .filter(|latency| latency.is_finite())
            .collect::<Vec<_>>();
        if latencies.is_empty() {
            0.0
        } else {
            latencies.iter().sum::<f64>() / latencies.len() as f64
        }
    };

    let previous_requests = previous.requests as f64;
    let previous_tokens = previous.total_tokens as f64;
    let previous_latency = previous.avg_latency_ms;
    let previous_errors = previous.error_count as f64;
    let previous_success_rate = if previous_requests > 0.0 {
        ((previous_requests - previous_errors) / previous_requests) * 100.0
    } else {
        0.0
    };

    DashboardOverview {
        total_requests,
        total_tokens,
        input_tokens,
        output_tokens,
        avg_latency_ms,
        success_rate,
        delta: DashboardOverviewDelta {
            total_requests: percent_change(total_requests as f64, previous_requests),
            total_tokens: percent_change(total_tokens as f64, previous_tokens),
            avg_latency_ms: percent_change(avg_latency_ms, previous_latency),
            success_rate: percent_change(success_rate, previous_success_rate),
        },
    }
}

pub(super) fn bucket_key(bucket: &DashboardBucket, timestamp: NaiveDateTime) -> String {
    match bucket {
        DashboardBucket::Hour => timestamp.format("%Y-%m-%d %H:00:00").to_string(),
        DashboardBucket::Day => timestamp.format("%Y-%m-%d").to_string(),
    }
}

pub(super) fn bucket_label(bucket: &DashboardBucket, timestamp: NaiveDateTime) -> String {
    match bucket {
        DashboardBucket::Hour => timestamp.format("%H:%M").to_string(),
        DashboardBucket::Day => timestamp.format("%m-%d").to_string(),
    }
}

pub(super) fn iter_bucket_points(window: &DashboardWindow) -> Vec<(String, String)> {
    let mut current = window.current_start;
    let step = match window.bucket {
        DashboardBucket::Hour => ChronoDuration::hours(1),
        DashboardBucket::Day => ChronoDuration::days(1),
    };
    let mut items = Vec::new();

    while current <= window.current_end {
        items.push((
            bucket_key(&window.bucket, current),
            bucket_label(&window.bucket, current),
        ));
        current += step;
    }

    items
}

pub(super) fn build_trend_section(
    current_records: &[DashboardUsageRecord],
    window: &DashboardWindow,
) -> DashboardTrendSection {
    let mut buckets = BTreeMap::new();
    for (key, label) in iter_bucket_points(window) {
        buckets.insert(
            key,
            TrendAccumulator {
                label,
                ..TrendAccumulator::default()
            },
        );
    }

    for record in current_records {
        let Some(timestamp) = parse_dashboard_timestamp(&record.timestamp) else {
            continue;
        };
        let key = bucket_key(&window.bucket, timestamp);
        let entry = buckets.entry(key).or_insert_with(|| TrendAccumulator {
            label: bucket_label(&window.bucket, timestamp),
            ..TrendAccumulator::default()
        });
        entry.requests += 1;
        entry.input_tokens += record.input_tokens.max(0);
        entry.output_tokens += record.output_tokens.max(0);
        if usage_record_is_error(record) {
            entry.error_count += 1;
        }
        if let Some(latency) = record.latency_ms.filter(|latency| latency.is_finite()) {
            entry.latency_sum += latency;
            entry.latency_count += 1;
        }
    }

    let points = buckets
        .into_iter()
        .map(|(bucket, item)| {
            let total_tokens = item.input_tokens + item.output_tokens;
            let error_rate = if item.requests > 0 {
                (item.error_count as f64 / item.requests as f64) * 100.0
            } else {
                0.0
            };
            let avg_latency_ms = if item.latency_count > 0 {
                item.latency_sum / item.latency_count as f64
            } else {
                0.0
            };

            DashboardTrendPoint {
                bucket,
                label: item.label,
                requests: item.requests,
                input_tokens: item.input_tokens,
                output_tokens: item.output_tokens,
                total_tokens,
                error_rate,
                avg_latency_ms,
                success_rate: if item.requests > 0 {
                    100.0 - error_rate
                } else {
                    0.0
                },
            }
        })
        .collect();

    DashboardTrendSection {
        unit: match window.bucket {
            DashboardBucket::Hour => "hour".to_string(),
            DashboardBucket::Day => "day".to_string(),
        },
        points,
    }
}

pub(super) fn build_team_usage_section(
    records: &[DashboardUsageRecord],
) -> DashboardTeamUsageSection {
    let mut team_totals: HashMap<String, (i64, i64)> = HashMap::new();
    let mut team_model: HashMap<(String, String), (i64, i64)> = HashMap::new();

    for record in records {
        let total_tokens = usage_record_total_tokens(record);
        let team_entry = team_totals.entry(record.team_id.clone()).or_insert((0, 0));
        team_entry.0 += 1;
        team_entry.1 += total_tokens;

        let model_entry = team_model
            .entry((record.team_id.clone(), record.model.clone()))
            .or_insert((0, 0));
        model_entry.0 += 1;
        model_entry.1 += total_tokens;
    }

    let mut leaderboard = team_totals
        .into_iter()
        .map(
            |(team_id, (total_requests, total_tokens))| DashboardTeamLeaderboardItem {
                team_id,
                total_requests,
                total_tokens,
            },
        )
        .collect::<Vec<_>>();
    leaderboard.sort_by(|left, right| {
        right
            .total_tokens
            .cmp(&left.total_tokens)
            .then_with(|| right.total_requests.cmp(&left.total_requests))
    });
    leaderboard.truncate(10);

    let mut model_usage = team_model
        .into_iter()
        .map(
            |((team_id, model), (total_requests, total_tokens))| DashboardTeamModelUsageItem {
                team_id,
                model,
                total_requests,
                total_tokens,
            },
        )
        .collect::<Vec<_>>();
    model_usage.sort_by(|left, right| {
        left.team_id
            .cmp(&right.team_id)
            .then_with(|| right.total_tokens.cmp(&left.total_tokens))
    });

    DashboardTeamUsageSection {
        leaderboard,
        model_usage,
    }
}

pub(super) fn build_system_reliability_section(
    records: &[DashboardUsageRecord],
    trend: &DashboardTrendSection,
) -> DashboardSystemReliabilitySection {
    let mut channel_latency: HashMap<String, Vec<f64>> = HashMap::new();

    for record in records {
        if let Some(latency) = record.latency_ms.filter(|latency| latency.is_finite()) {
            channel_latency
                .entry(record.final_channel.clone())
                .or_default()
                .push(latency);
        }
    }

    let mut channel_items = channel_latency
        .into_iter()
        .map(|(channel, mut latencies)| {
            let total_requests = latencies.len() as i64;
            let avg_latency_ms = if latencies.is_empty() {
                0.0
            } else {
                latencies.iter().sum::<f64>() / latencies.len() as f64
            };
            let p95_latency_ms = percentile(&mut latencies, 0.95);
            DashboardChannelLatencyItem {
                channel,
                total_requests,
                avg_latency_ms,
                p95_latency_ms,
            }
        })
        .collect::<Vec<_>>();
    channel_items.sort_by(|left, right| {
        right
            .avg_latency_ms
            .total_cmp(&left.avg_latency_ms)
            .then_with(|| left.channel.cmp(&right.channel))
    });

    DashboardSystemReliabilitySection {
        error_rate_trend: trend.points.clone(),
        channel_latency: channel_items,
    }
}

/// Per-client (tool) usage breakdown. Records without a detected client are
/// bucketed under "Unknown" (e.g. failed requests, tools that send no UA).
pub(super) fn build_client_usage_section(
    records: &[DashboardUsageRecord],
) -> Vec<DashboardShareItem> {
    let total_requests = records.len() as f64;
    let mut map: HashMap<String, (i64, i64)> = HashMap::new();
    for record in records {
        let total_tokens = usage_record_total_tokens(record);
        let key = record
            .client
            .clone()
            .filter(|c| !c.is_empty())
            .unwrap_or_else(|| "Unknown".to_string());
        map.entry(key)
            .and_modify(|entry| {
                entry.0 += 1;
                entry.1 += total_tokens;
            })
            .or_insert((1, total_tokens));
    }

    let mut items = map
        .into_iter()
        .map(|(name, (requests, total_tokens))| DashboardShareItem {
            name,
            requests,
            total_tokens,
            percentage: if total_requests > 0.0 {
                (requests as f64 / total_requests) * 100.0
            } else {
                0.0
            },
        })
        .collect::<Vec<_>>();
    items.sort_by_key(|item| std::cmp::Reverse(item.requests));
    items
}

pub(super) fn build_model_router_section(
    records: &[DashboardUsageRecord],
) -> DashboardModelRouterSection {
    let total_requests = records.len() as f64;
    let mut model_map: HashMap<String, (i64, i64)> = HashMap::new();
    let mut router_map: HashMap<String, (i64, i64)> = HashMap::new();
    let mut channel_map: HashMap<String, (i64, i64)> = HashMap::new();

    for record in records {
        let total_tokens = usage_record_total_tokens(record);
        model_map
            .entry(record.model.clone())
            .and_modify(|entry| {
                entry.0 += 1;
                entry.1 += total_tokens;
            })
            .or_insert((1, total_tokens));
        router_map
            .entry(record.router.clone())
            .and_modify(|entry| {
                entry.0 += 1;
                entry.1 += total_tokens;
            })
            .or_insert((1, total_tokens));
        channel_map
            .entry(record.final_channel.clone())
            .and_modify(|entry| {
                entry.0 += 1;
                entry.1 += total_tokens;
            })
            .or_insert((1, total_tokens));
    }

    let to_items = |map: HashMap<String, (i64, i64)>| {
        let mut items = map
            .into_iter()
            .map(|(name, (requests, total_tokens))| DashboardShareItem {
                name,
                requests,
                total_tokens,
                percentage: if total_requests > 0.0 {
                    (requests as f64 / total_requests) * 100.0
                } else {
                    0.0
                },
            })
            .collect::<Vec<_>>();
        items.sort_by_key(|item| std::cmp::Reverse(item.requests));
        items
    };

    DashboardModelRouterSection {
        model_share: to_items(model_map),
        router_summary: to_items(router_map),
        channel_summary: to_items(channel_map),
    }
}

pub(super) fn build_topology_section(records: &[DashboardUsageRecord]) -> DashboardTopologySection {
    let mut flow_map: HashMap<(String, String, String, String), (i64, i64)> = HashMap::new();
    for record in records {
        let key = (
            record.team_id.clone(),
            record.router.clone(),
            record.final_channel.clone(),
            record.model.clone(),
        );
        let total_tokens = usage_record_total_tokens(record);
        flow_map
            .entry(key)
            .and_modify(|entry| {
                entry.0 += 1;
                entry.1 += total_tokens;
            })
            .or_insert((1, total_tokens));
    }

    let mut flows = flow_map
        .into_iter()
        .map(
            |((team_id, router, channel, model), (requests, total_tokens))| DashboardFlowSummary {
                team_id,
                router,
                channel,
                model,
                requests,
                total_tokens,
            },
        )
        .collect::<Vec<_>>();
    flows.sort_by_key(|flow| std::cmp::Reverse(flow.requests));

    let mut nodes = Vec::new();
    let mut node_index: HashMap<(String, String), usize> = HashMap::new();
    let mut ensure_node = |name: &str, kind: &str| -> usize {
        let key = (kind.to_string(), name.to_string());
        if let Some(index) = node_index.get(&key) {
            return *index;
        }

        let index = nodes.len();
        nodes.push(DashboardTopologyNode {
            name: name.to_string(),
            kind: kind.to_string(),
        });
        node_index.insert(key, index);
        index
    };

    let mut link_values: HashMap<(usize, usize), (i64, i64)> = HashMap::new();
    for flow in &flows {
        let team_idx = ensure_node(&flow.team_id, "team");
        let router_idx = ensure_node(&flow.router, "router");
        let channel_idx = ensure_node(&flow.channel, "channel");
        let model_idx = ensure_node(&flow.model, "model");

        for pair in [
            (team_idx, router_idx),
            (router_idx, channel_idx),
            (channel_idx, model_idx),
        ] {
            link_values
                .entry(pair)
                .and_modify(|value| {
                    value.0 += flow.requests;
                    value.1 += flow.total_tokens;
                })
                .or_insert((flow.requests, flow.total_tokens));
        }
    }

    let links = link_values
        .into_iter()
        .map(
            |((source, target), (value, total_tokens))| DashboardTopologyLink {
                source,
                target,
                value,
                total_tokens,
            },
        )
        .collect::<Vec<_>>();

    DashboardTopologySection {
        nodes,
        links,
        flows,
        render_mode: "sankey".to_string(),
    }
}

pub(super) fn latest_cursor(records: &[DashboardUsageRecord]) -> Option<DashboardRecordCursor> {
    records.first().map(|record| DashboardRecordCursor {
        id: record.id,
        timestamp: record.timestamp.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn topology_keeps_same_name_in_different_dimensions_separate() {
        let records = vec![DashboardUsageRecord {
            id: 1,
            timestamp: "2026-03-12 12:00:00".to_string(),
            request_id: Some("req-1".to_string()),
            team_id: "default".to_string(),
            router: "default".to_string(),
            matched_rule: Some("*".to_string()),
            final_channel: "openai".to_string(),
            channel: "openai".to_string(),
            model: "gpt-4o".to_string(),
            input_tokens: 10,
            output_tokens: 20,
            latency_ms: Some(100.0),
            fallback_triggered: false,
            status: "success".to_string(),
            status_code: Some(200),
            error_message: None,
            provider_trace_id: None,
            provider_error_body: None,
            client: None,
            user_agent: None,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
            req_hash: None,
            session_key: None,
        }];

        let topology = build_topology_section(&records);
        let default_nodes = topology
            .nodes
            .iter()
            .filter(|node| node.name == "default")
            .collect::<Vec<_>>();

        assert_eq!(default_nodes.len(), 2);
        assert!(default_nodes.iter().any(|node| node.kind == "team"));
        assert!(default_nodes.iter().any(|node| node.kind == "router"));
    }

    #[test]
    fn topology_links_include_aggregated_total_tokens() {
        let records = vec![
            DashboardUsageRecord {
                id: 1,
                timestamp: "2026-03-12 12:00:00".to_string(),
                request_id: Some("req-1".to_string()),
                team_id: "team-alpha".to_string(),
                router: "default".to_string(),
                matched_rule: Some("*".to_string()),
                final_channel: "openai".to_string(),
                channel: "openai".to_string(),
                model: "gpt-4o".to_string(),
                input_tokens: 10,
                output_tokens: 20,
                latency_ms: Some(100.0),
                fallback_triggered: false,
                status: "success".to_string(),
                status_code: Some(200),
                error_message: None,
                provider_trace_id: None,
                provider_error_body: None,
                client: None,
                user_agent: None,
                cache_read_tokens: 0,
                cache_write_tokens: 0,
                req_hash: None,
                session_key: None,
            },
            DashboardUsageRecord {
                id: 2,
                timestamp: "2026-03-12 12:01:00".to_string(),
                request_id: Some("req-2".to_string()),
                team_id: "team-alpha".to_string(),
                router: "default".to_string(),
                matched_rule: Some("*".to_string()),
                final_channel: "openai".to_string(),
                channel: "openai".to_string(),
                model: "gpt-4o".to_string(),
                input_tokens: 15,
                output_tokens: 25,
                latency_ms: Some(120.0),
                fallback_triggered: false,
                status: "success".to_string(),
                status_code: Some(200),
                error_message: None,
                provider_trace_id: None,
                provider_error_body: None,
                client: None,
                user_agent: None,
                cache_read_tokens: 0,
                cache_write_tokens: 0,
                req_hash: None,
                session_key: None,
            },
        ];

        let topology = build_topology_section(&records);
        let team_link = topology
            .links
            .iter()
            .find(|link| {
                topology.nodes[link.source].name == "team-alpha"
                    && topology.nodes[link.target].name == "default"
            })
            .expect("team -> router link should exist");

        assert_eq!(team_link.value, 2);
        assert_eq!(team_link.total_tokens, 70);
    }

    #[test]
    fn team_usage_leaderboard_is_capped_at_top_ten() {
        let records = (0..12)
            .map(|index| DashboardUsageRecord {
                id: index + 1,
                timestamp: "2026-03-12 12:00:00".to_string(),
                request_id: Some(format!("req-{index}")),
                team_id: format!("team-{index:02}"),
                router: "default".to_string(),
                matched_rule: Some("*".to_string()),
                final_channel: "openai".to_string(),
                channel: "openai".to_string(),
                model: "gpt-4o".to_string(),
                input_tokens: 10,
                output_tokens: 100 - index,
                latency_ms: Some(100.0),
                fallback_triggered: false,
                status: "success".to_string(),
                status_code: Some(200),
                error_message: None,
                provider_trace_id: None,
                provider_error_body: None,
                client: None,
                user_agent: None,
                cache_read_tokens: 0,
                cache_write_tokens: 0,
                req_hash: None,
                session_key: None,
            })
            .collect::<Vec<_>>();

        let team_usage = build_team_usage_section(&records);

        assert_eq!(team_usage.leaderboard.len(), 10);
        assert_eq!(
            team_usage
                .leaderboard
                .first()
                .map(|item| item.team_id.as_str()),
            Some("team-00")
        );
        assert_eq!(
            team_usage
                .leaderboard
                .last()
                .map(|item| item.team_id.as_str()),
            Some("team-09")
        );
    }
}
