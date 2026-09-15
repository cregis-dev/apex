//! The control plane's dashboard API: two read-only endpoints that query usage
//! records and aggregate them into the sections the CP renders.
//!
//! The wire types live here rather than in a sibling module so the submodules
//! that build them can construct them directly — a private field is visible to
//! descendants of the defining module, not to its siblings.

mod behavior;
mod cost;
mod sections;

use std::collections::HashMap;
use std::sync::Arc;

use axum::body::Body;
use axum::extract::State;
use axum::http::{Response, StatusCode};
use chrono::{Duration as ChronoDuration, Local, NaiveDateTime};

use crate::database::{UsageRecord as DashboardUsageRecord, UsageRecordPage, UsageRecordQuery};

use super::AppState;

use behavior::{BEHAVIOR_BASELINE_DAYS, build_behavior_section};
use cost::build_cost_section;
use sections::*;

#[derive(Debug, Clone)]
enum DashboardBucket {
    Hour,
    Day,
}

#[derive(Debug, Clone)]
struct DashboardWindow {
    range: String,
    bucket: DashboardBucket,
    current_start: NaiveDateTime,
    current_end: NaiveDateTime,
    previous_start: NaiveDateTime,
    previous_end: NaiveDateTime,
}

#[derive(Debug, Clone, serde::Serialize)]
struct DashboardRecordCursor {
    id: i64,
    timestamp: String,
}

#[derive(Debug, Clone, serde::Serialize)]
struct DashboardFilterOptions {
    teams: Vec<String>,
    models: Vec<String>,
    routers: Vec<String>,
    channels: Vec<String>,
    clients: Vec<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
struct DashboardOverviewDelta {
    total_requests: f64,
    total_tokens: f64,
    avg_latency_ms: f64,
    success_rate: f64,
}

#[derive(Debug, Clone, serde::Serialize)]
struct DashboardOverview {
    total_requests: i64,
    total_tokens: i64,
    input_tokens: i64,
    output_tokens: i64,
    avg_latency_ms: f64,
    success_rate: f64,
    delta: DashboardOverviewDelta,
}

#[derive(Debug, Clone, serde::Serialize)]
struct DashboardTrendPoint {
    bucket: String,
    label: String,
    requests: i64,
    input_tokens: i64,
    output_tokens: i64,
    total_tokens: i64,
    error_rate: f64,
    avg_latency_ms: f64,
    success_rate: f64,
}

#[derive(Debug, Clone, serde::Serialize)]
struct DashboardTrendSection {
    unit: String,
    points: Vec<DashboardTrendPoint>,
}

#[derive(Debug, Clone, serde::Serialize)]
struct DashboardTeamLeaderboardItem {
    team_id: String,
    total_requests: i64,
    total_tokens: i64,
}

#[derive(Debug, Clone, serde::Serialize)]
struct DashboardTeamModelUsageItem {
    team_id: String,
    model: String,
    total_requests: i64,
    total_tokens: i64,
}

#[derive(Debug, Clone, serde::Serialize)]
struct DashboardTeamUsageSection {
    leaderboard: Vec<DashboardTeamLeaderboardItem>,
    model_usage: Vec<DashboardTeamModelUsageItem>,
}

#[derive(Debug, Clone, serde::Serialize)]
struct DashboardChannelLatencyItem {
    channel: String,
    total_requests: i64,
    avg_latency_ms: f64,
    p95_latency_ms: f64,
}

#[derive(Debug, Clone, serde::Serialize)]
struct DashboardSystemReliabilitySection {
    error_rate_trend: Vec<DashboardTrendPoint>,
    channel_latency: Vec<DashboardChannelLatencyItem>,
}

#[derive(Debug, Clone, serde::Serialize)]
struct DashboardShareItem {
    name: String,
    requests: i64,
    total_tokens: i64,
    percentage: f64,
}

#[derive(Debug, Clone, serde::Serialize)]
struct DashboardModelRouterSection {
    model_share: Vec<DashboardShareItem>,
    router_summary: Vec<DashboardShareItem>,
    channel_summary: Vec<DashboardShareItem>,
}

// --- Cost section (present only when `pricing` is configured) ---

#[derive(Debug, Clone, serde::Serialize)]
struct DashboardCostMemberItem {
    /// User identity (wire `team_id`).
    id: String,
    /// The user's team (wire `group`), if any.
    group: Option<String>,
    actual_cost: f64,
    reference_cost: f64,
}

#[derive(Debug, Clone, serde::Serialize)]
struct DashboardCostModelItem {
    name: String,
    actual_cost: f64,
    reference_cost: f64,
}

#[derive(Debug, Clone, serde::Serialize)]
struct DashboardSubscriptionItem {
    /// Name of the subscription pricing rule.
    name: String,
    monthly_fee: f64,
    /// Fee accrued over this window (monthly_fee prorated by window duration).
    accrued_fee: f64,
    /// Total tokens (in + out + cache) used on this rule this window.
    tokens_used: f64,
    /// Included quota prorated to this window (0 when the rule has no quota).
    quota_tokens: f64,
    /// tokens_used / quota_tokens (0 when no quota set). >1 = over quota.
    utilization: f64,
    /// Accrued fee left unallocated because the rule had no traffic (pure waste).
    idle_fee: f64,
}

#[derive(Debug, Clone, serde::Serialize)]
struct DashboardCostSection {
    currency: String,
    /// Real money out the door: PAYG reference cost + accrued subscription fees.
    actual_cost: f64,
    /// What all traffic would cost at standard PAYG list rates.
    reference_cost: f64,
    /// Percent change in actual_cost vs the previous window.
    delta_actual_cost: f64,
    /// 1 - actual/reference (how much cheaper than list PAYG). 0 if no reference.
    effective_discount: f64,
    by_member: Vec<DashboardCostMemberItem>,
    by_model: Vec<DashboardCostModelItem>,
    subscriptions: Vec<DashboardSubscriptionItem>,
}

// --- Behavior section (present only when `profiling.enabled`) ---

/// One tripped detection signal for a member, with the evidence that tripped it
/// and a suggested (never auto-applied) governance action.
#[derive(Debug, Clone, serde::Serialize)]
struct DashboardBehaviorFlag {
    /// One of: repeat_rate, rate_spike, error_storm, output_zero, spend_spike, off_hours.
    signal: String,
    /// warning | critical.
    severity: String,
    /// The metric value that tripped the flag.
    value: f64,
    /// The configured threshold it exceeded.
    threshold: f64,
    /// Human-readable evidence (counts, z-scores) — no prompt content.
    detail: String,
    /// observe | rate_limit | disable — a suggestion for the operator to confirm.
    suggested_action: String,
}

/// A member's raw behavior metrics over the window (the numbers the flags read).
#[derive(Debug, Clone, serde::Serialize)]
struct DashboardBehaviorProfile {
    requests: i64,
    repeat_rate: f64,
    error_ratio: f64,
    output_zero_ratio: f64,
    night_ratio: f64,
    /// Request-rate z-score vs the member's own rolling baseline (None if the
    /// baseline is too sparse to be meaningful).
    #[serde(skip_serializing_if = "Option::is_none")]
    rate_z: Option<f64>,
    /// Reference-cost z-score vs baseline (None without pricing or baseline).
    #[serde(skip_serializing_if = "Option::is_none")]
    spend_z: Option<f64>,
    /// Window reference cost (None when pricing is not configured).
    #[serde(skip_serializing_if = "Option::is_none")]
    reference_cost: Option<f64>,
}

/// A flagged member: their profile plus the signals that tripped, ranked by
/// severity in the section.
#[derive(Debug, Clone, serde::Serialize)]
struct DashboardBehaviorMember {
    /// User identity (wire `team_id`).
    id: String,
    /// The user's team (wire `group`), if any.
    group: Option<String>,
    /// Highest severity across this member's flags.
    severity: String,
    /// Escalated suggested action across this member's flags.
    suggested_action: String,
    profile: DashboardBehaviorProfile,
    flags: Vec<DashboardBehaviorFlag>,
}

/// Read-time abuse/waste detection over the window. Present only when profiling
/// is enabled; members with no flags are omitted.
#[derive(Debug, Clone, serde::Serialize)]
struct DashboardBehaviorSection {
    window_secs: f64,
    /// Members with enough samples this window to be judged.
    evaluated: usize,
    /// Of those, how many tripped at least one flag.
    flagged: usize,
    /// Flagged members only, most severe first.
    members: Vec<DashboardBehaviorMember>,
}

#[derive(Debug, Clone, serde::Serialize)]
struct DashboardTopologyNode {
    name: String,
    kind: String,
}

#[derive(Debug, Clone, serde::Serialize)]
struct DashboardTopologyLink {
    source: usize,
    target: usize,
    value: i64,
    total_tokens: i64,
}

#[derive(Debug, Clone, serde::Serialize)]
struct DashboardFlowSummary {
    team_id: String,
    router: String,
    channel: String,
    model: String,
    requests: i64,
    total_tokens: i64,
}

#[derive(Debug, Clone, serde::Serialize)]
struct DashboardTopologySection {
    nodes: Vec<DashboardTopologyNode>,
    links: Vec<DashboardTopologyLink>,
    flows: Vec<DashboardFlowSummary>,
    render_mode: String,
}

#[derive(Debug, Clone, serde::Serialize)]
struct DashboardRecordsMeta {
    total: usize,
    latest_cursor: Option<DashboardRecordCursor>,
}

#[derive(Debug, Clone, serde::Serialize)]
struct DashboardAnalyticsResponse {
    generated_at: String,
    range: String,
    filter_options: DashboardFilterOptions,
    overview: DashboardOverview,
    trend: DashboardTrendSection,
    topology: DashboardTopologySection,
    team_usage: DashboardTeamUsageSection,
    system_reliability: DashboardSystemReliabilitySection,
    model_router: DashboardModelRouterSection,
    client_usage: Vec<DashboardShareItem>,
    /// Cost breakdown. Present only when `pricing` is configured; omitted otherwise
    /// so the dashboard can hide cost views rather than show a misleading $0.
    #[serde(skip_serializing_if = "Option::is_none")]
    cost: Option<DashboardCostSection>,
    /// Behavior profiling / abuse detection. Present only when `profiling` is
    /// enabled; omitted otherwise so the dashboard hides governance views.
    #[serde(skip_serializing_if = "Option::is_none")]
    behavior: Option<DashboardBehaviorSection>,
    records_meta: DashboardRecordsMeta,
}

#[derive(Debug, Clone, serde::Serialize)]
struct DashboardRecordsResponse {
    data: Vec<DashboardUsageRecord>,
    total: usize,
    limit: usize,
    offset: usize,
    latest_cursor: Option<DashboardRecordCursor>,
    new_records: usize,
}

#[derive(Default)]
struct TrendAccumulator {
    label: String,
    requests: i64,
    input_tokens: i64,
    output_tokens: i64,
    error_count: i64,
    latency_sum: f64,
    latency_count: i64,
}

pub(super) async fn dashboard_analytics_api_handler(
    State(state): State<Arc<AppState>>,
    axum::extract::Query(params): axum::extract::Query<HashMap<String, String>>,
) -> Response<Body> {
    let window = dashboard_window(params.get("range").map(String::as_str));
    let query = build_dashboard_usage_query(&params, window.current_start, window.current_end);
    let previous_query =
        build_dashboard_usage_query(&params, window.previous_start, window.previous_end);
    let options_query = UsageRecordQuery {
        start_time: Some(format_dashboard_timestamp(window.current_start)),
        end_time: Some(format_dashboard_timestamp(window.current_end)),
        ..UsageRecordQuery::default()
    };

    let current_records = match state.database.get_usage_records_for_analytics(&query) {
        Ok(records) => records,
        Err(err) => {
            return Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .body(Body::from(err.to_string()))
                .unwrap();
        }
    };
    // Period-over-period deltas only need aggregates of the previous window, and
    // the filter dropdowns only need its distinct values — compute both in SQL
    // instead of loading every row of those windows into memory.
    let previous = match state.database.get_usage_aggregate(&previous_query) {
        Ok(aggregate) => aggregate,
        Err(err) => {
            return Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .body(Body::from(err.to_string()))
                .unwrap();
        }
    };
    let filter_options = match state.database.get_filter_options(&options_query) {
        Ok(options) => DashboardFilterOptions {
            teams: options.teams,
            models: options.models,
            routers: options.routers,
            channels: options.channels,
            clients: options.clients,
        },
        Err(err) => {
            return Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .body(Body::from(err.to_string()))
                .unwrap();
        }
    };

    // Cost section only when pricing is configured. Snapshot the config bits we
    // need (cheap Arc clones) so we don't hold the RwLock across the DB read.
    let (pricing_opt, profiling_opt, channels_arc, teams_arc) = {
        let config = state.config.read().unwrap();
        (
            config.pricing.clone(),
            config.profiling.clone(),
            config.channels.clone(),
            config.teams.clone(),
        )
    };
    // Any entity filter active? (range only defines the window, not a filter.)
    let filtered = ["team_id", "router", "channel", "model", "client", "status"]
        .iter()
        .any(|k| {
            params
                .get(*k)
                .map(|v| !v.trim().is_empty() && !v.eq_ignore_ascii_case("all"))
                .unwrap_or(false)
        });
    let window_secs = (window.current_end - window.current_start).num_seconds() as f64;
    let cost = pricing_opt.as_ref().map(|pricing| {
        // Deltas need the previous window's rows (priced per model), which the
        // aggregate above can't give — fetch them only when cost is enabled.
        let previous_records = state
            .database
            .get_usage_records_for_analytics(&previous_query)
            .unwrap_or_default();
        build_cost_section(
            &current_records,
            &previous_records,
            pricing,
            &channels_arc,
            &teams_arc,
            window_secs,
            filtered,
        )
    });
    // Behavior profiling / abuse detection — read-time, only when enabled. Reads
    // the member's rolling rollup baseline (the 7d before the window) to z-score
    // rate/spend; window rule signals need no baseline.
    let behavior = profiling_opt.filter(|p| p.enabled).map(|p| {
        let baseline_start = window.current_start - ChronoDuration::days(BEHAVIOR_BASELINE_DAYS);
        // Floor both bounds to the hour so the half-open range actually excludes
        // the window's own start-hour bucket. `get_rollup_between` is `start <=
        // bucket < end`, but rollup bucket_start is hour-floored ("14:00:00"),
        // so an un-floored end ("14:37:12") would still admit the 14:00 bucket —
        // which aggregates the window's own in-hour traffic and pollutes its
        // baseline. Flooring `end` to "14:00:00" drops that bucket.
        let hour_floor = |t: NaiveDateTime| t.format("%Y-%m-%d %H:00:00").to_string();
        let baseline = state
            .database
            .get_rollup_between(
                &hour_floor(baseline_start),
                &hour_floor(window.current_start),
            )
            .unwrap_or_default();
        build_behavior_section(
            &current_records,
            &baseline,
            &p.thresholds,
            &teams_arc,
            pricing_opt.as_ref(),
            &channels_arc,
            &window,
        )
    });

    let trend = build_trend_section(&current_records, &window);
    let response = DashboardAnalyticsResponse {
        generated_at: format_dashboard_timestamp(Local::now().naive_local()),
        range: window.range,
        filter_options,
        overview: build_overview(&current_records, &previous),
        topology: build_topology_section(&current_records),
        team_usage: build_team_usage_section(&current_records),
        system_reliability: build_system_reliability_section(&current_records, &trend),
        model_router: build_model_router_section(&current_records),
        client_usage: build_client_usage_section(&current_records),
        cost,
        behavior,
        records_meta: DashboardRecordsMeta {
            total: current_records.len(),
            latest_cursor: latest_cursor(&current_records),
        },
        trend,
    };

    Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "application/json")
        .body(Body::from(
            serde_json::to_string(&response).unwrap_or_default(),
        ))
        .unwrap()
}

pub(super) async fn dashboard_records_api_handler(
    State(state): State<Arc<AppState>>,
    axum::extract::Query(params): axum::extract::Query<HashMap<String, String>>,
) -> Response<Body> {
    let window = dashboard_window(params.get("range").map(String::as_str));
    let query = build_dashboard_usage_query(&params, window.current_start, window.current_end);
    let limit = params
        .get("limit")
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(20)
        .min(100);
    let offset = params
        .get("offset")
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(0);
    let since_timestamp = params.get("since_timestamp").map(String::as_str);
    let since_id = params
        .get("since_id")
        .and_then(|value| value.parse::<i64>().ok());

    match state.database.get_usage_records_page(
        &query,
        limit as i64,
        offset as i64,
        since_timestamp,
        since_id,
    ) {
        Ok(UsageRecordPage {
            records,
            total,
            new_records,
            latest_cursor,
        }) => {
            let payload = DashboardRecordsResponse {
                data: records,
                total: total as usize,
                limit,
                offset,
                latest_cursor: latest_cursor
                    .map(|(id, timestamp)| DashboardRecordCursor { id, timestamp }),
                new_records: new_records as usize,
            };

            Response::builder()
                .status(StatusCode::OK)
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_string(&payload).unwrap_or_default(),
                ))
                .unwrap()
        }
        Err(err) => Response::builder()
            .status(StatusCode::INTERNAL_SERVER_ERROR)
            .body(Body::from(err.to_string()))
            .unwrap(),
    }
}
