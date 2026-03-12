use crate::database::{Database, UsageRecord as DbUsageRecord};
use anyhow::Result;
use chrono::{DateTime, NaiveDate, NaiveDateTime};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

/// Export-friendly usage record.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct UsageRecord {
    pub timestamp: String,
    pub team_id: String,
    pub router: String,
    pub channel: String,
    pub model: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub latency_ms: Option<f64>,
    pub fallback_triggered: bool,
    pub status: String,
}

/// Query filters for analytics.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UsageQuery {
    pub team_id: Option<String>,
    pub router: Option<String>,
    pub channel: Option<String>,
    pub model: Option<String>,
    pub status: Option<String>,
    pub start_time: Option<String>,
    pub end_time: Option<String>,
}

/// Aggregated usage statistics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageStats {
    pub total_requests: u64,
    pub total_input_tokens: u64,
    pub total_output_tokens: u64,
    pub total_tokens: u64,
    pub avg_latency_ms: Option<f64>,
    pub p50_latency_ms: Option<f64>,
    pub p95_latency_ms: Option<f64>,
    pub p99_latency_ms: Option<f64>,
    pub error_rate: Option<f64>,
    pub by_router: HashMap<String, RouterStats>,
    pub by_channel: HashMap<String, ChannelStats>,
    pub by_model: HashMap<String, ModelStats>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub by_team: Option<HashMap<String, TeamStats>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouterStats {
    pub total_requests: u64,
    pub total_tokens: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelStats {
    pub total_requests: u64,
    pub total_tokens: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelStats {
    pub total_requests: u64,
    pub total_tokens: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamStats {
    pub total_requests: u64,
    pub total_tokens: u64,
    pub routers_used: Vec<String>,
}

pub struct AnalyticsEngine {
    db: Arc<Database>,
}

impl AnalyticsEngine {
    pub fn new(db: Arc<Database>) -> Self {
        Self { db }
    }

    fn parse_timestamp(&self, ts: &str) -> Option<NaiveDateTime> {
        NaiveDateTime::parse_from_str(ts, "%Y-%m-%d %H:%M:%S")
            .or_else(|_| NaiveDateTime::parse_from_str(ts, "%Y-%m-%dT%H:%M:%S"))
            .or_else(|_| DateTime::parse_from_rfc3339(ts).map(|dt| dt.naive_utc()))
            .ok()
    }

    fn normalize_time_bound(&self, raw: &str, is_end: bool) -> Option<String> {
        let raw = raw.trim();

        if let Ok(date) = NaiveDate::parse_from_str(raw, "%Y-%m-%d") {
            let time = if is_end {
                date.and_hms_opt(23, 59, 59)?
            } else {
                date.and_hms_opt(0, 0, 0)?
            };
            return Some(time.format("%Y-%m-%d %H:%M:%S").to_string());
        }

        self.parse_timestamp(raw)
            .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
    }

    fn normalized_bounds(&self, query: &UsageQuery) -> (Option<String>, Option<String>) {
        (
            query
                .start_time
                .as_deref()
                .and_then(|ts| self.normalize_time_bound(ts, false)),
            query
                .end_time
                .as_deref()
                .and_then(|ts| self.normalize_time_bound(ts, true)),
        )
    }

    fn to_export_record(record: &DbUsageRecord) -> UsageRecord {
        UsageRecord {
            timestamp: record.timestamp.clone(),
            team_id: record.team_id.clone(),
            router: record.router.clone(),
            channel: record.channel.clone(),
            model: record.model.clone(),
            input_tokens: record.input_tokens.max(0) as u64,
            output_tokens: record.output_tokens.max(0) as u64,
            latency_ms: record.latency_ms,
            fallback_triggered: record.fallback_triggered,
            status: record.status.clone(),
        }
    }

    fn total_tokens(record: &DbUsageRecord) -> u64 {
        record.input_tokens.max(0) as u64 + record.output_tokens.max(0) as u64
    }

    fn is_error_status(status: &str) -> bool {
        matches!(status, "error" | "fallback_error")
    }

    fn percentile(values: &[f64], percentile: f64) -> Option<f64> {
        if values.is_empty() {
            return None;
        }

        let rank = ((values.len() - 1) as f64 * percentile.clamp(0.0, 1.0)).floor() as usize;
        values.get(rank).copied()
    }

    fn empty_stats(by_team: Option<HashMap<String, TeamStats>>) -> UsageStats {
        UsageStats {
            total_requests: 0,
            total_input_tokens: 0,
            total_output_tokens: 0,
            total_tokens: 0,
            avg_latency_ms: None,
            p50_latency_ms: None,
            p95_latency_ms: None,
            p99_latency_ms: None,
            error_rate: None,
            by_router: HashMap::new(),
            by_channel: HashMap::new(),
            by_model: HashMap::new(),
            by_team,
        }
    }

    fn aggregate_records(
        &self,
        records: &[DbUsageRecord],
        by_team: Option<HashMap<String, TeamStats>>,
    ) -> UsageStats {
        if records.is_empty() {
            return Self::empty_stats(by_team);
        }

        let total_requests = records.len() as u64;
        let total_input_tokens: u64 = records.iter().map(|r| r.input_tokens.max(0) as u64).sum();
        let total_output_tokens: u64 = records.iter().map(|r| r.output_tokens.max(0) as u64).sum();
        let total_tokens = total_input_tokens + total_output_tokens;

        let mut latencies = records
            .iter()
            .filter_map(|r| r.latency_ms)
            .filter(|latency| latency.is_finite())
            .collect::<Vec<_>>();
        latencies.sort_by(f64::total_cmp);

        let avg_latency_ms = if latencies.is_empty() {
            None
        } else {
            Some(latencies.iter().sum::<f64>() / latencies.len() as f64)
        };

        let error_count = records
            .iter()
            .filter(|record| Self::is_error_status(&record.status))
            .count() as u64;

        let mut by_router = HashMap::new();
        let mut by_channel = HashMap::new();
        let mut by_model = HashMap::new();

        for record in records {
            let token_count = Self::total_tokens(record);

            let router_entry = by_router
                .entry(record.router.clone())
                .or_insert(RouterStats {
                    total_requests: 0,
                    total_tokens: 0,
                });
            router_entry.total_requests += 1;
            router_entry.total_tokens += token_count;

            let channel_entry = by_channel
                .entry(record.channel.clone())
                .or_insert(ChannelStats {
                    total_requests: 0,
                    total_tokens: 0,
                });
            channel_entry.total_requests += 1;
            channel_entry.total_tokens += token_count;

            let model_entry = by_model.entry(record.model.clone()).or_insert(ModelStats {
                total_requests: 0,
                total_tokens: 0,
            });
            model_entry.total_requests += 1;
            model_entry.total_tokens += token_count;
        }

        UsageStats {
            total_requests,
            total_input_tokens,
            total_output_tokens,
            total_tokens,
            avg_latency_ms,
            p50_latency_ms: Self::percentile(&latencies, 0.50),
            p95_latency_ms: Self::percentile(&latencies, 0.95),
            p99_latency_ms: Self::percentile(&latencies, 0.99),
            error_rate: Some((error_count as f64 / total_requests as f64) * 100.0),
            by_router,
            by_channel,
            by_model,
            by_team,
        }
    }

    pub fn query_usage(&self, query: &UsageQuery) -> Result<Vec<UsageRecord>> {
        let (start_time, end_time) = self.normalized_bounds(query);
        let records = self.db.get_usage_records_for_analytics(
            query.team_id.as_deref(),
            query.router.as_deref(),
            query.channel.as_deref(),
            query.model.as_deref(),
            query.status.as_deref(),
            start_time.as_deref(),
            end_time.as_deref(),
        )?;

        Ok(records.iter().map(Self::to_export_record).collect())
    }

    pub fn query_usage_page(
        &self,
        query: &UsageQuery,
        limit: usize,
        offset: usize,
    ) -> Result<(Vec<UsageRecord>, usize)> {
        let records = self.query_usage(query)?;
        let total = records.len();
        let page = records.into_iter().skip(offset).take(limit).collect();
        Ok((page, total))
    }

    pub fn get_stats(&self, query: &UsageQuery) -> Result<UsageStats> {
        let (start_time, end_time) = self.normalized_bounds(query);
        let records = self.db.get_usage_records_for_analytics(
            query.team_id.as_deref(),
            query.router.as_deref(),
            query.channel.as_deref(),
            query.model.as_deref(),
            query.status.as_deref(),
            start_time.as_deref(),
            end_time.as_deref(),
        )?;

        Ok(self.aggregate_records(&records, None))
    }

    pub fn query_team_usage(
        &self,
        team_id: &str,
        team_routers: &[String],
        query: &UsageQuery,
    ) -> Result<UsageStats> {
        if team_routers.is_empty() {
            let mut by_team = HashMap::new();
            by_team.insert(
                team_id.to_string(),
                TeamStats {
                    total_requests: 0,
                    total_tokens: 0,
                    routers_used: Vec::new(),
                },
            );
            return Ok(Self::empty_stats(Some(by_team)));
        }

        if let Some(router) = query.router.as_deref()
            && !team_routers.iter().any(|allowed| allowed == router)
        {
            let mut by_team = HashMap::new();
            by_team.insert(
                team_id.to_string(),
                TeamStats {
                    total_requests: 0,
                    total_tokens: 0,
                    routers_used: Vec::new(),
                },
            );
            return Ok(Self::empty_stats(Some(by_team)));
        }

        let (start_time, end_time) = self.normalized_bounds(query);
        let records = self.db.get_usage_records_for_analytics(
            None,
            query.router.as_deref(),
            query.channel.as_deref(),
            query.model.as_deref(),
            query.status.as_deref(),
            start_time.as_deref(),
            end_time.as_deref(),
        )?;

        let team_records = records
            .into_iter()
            .filter(|record| {
                let team_matches = record.team_id == team_id || record.team_id.is_empty();
                let router_matches = team_routers.iter().any(|allowed| allowed == &record.router);
                team_matches && router_matches
            })
            .collect::<Vec<_>>();

        let routers_used = team_records.iter().fold(Vec::new(), |mut acc, record| {
            if !acc.iter().any(|router| router == &record.router) {
                acc.push(record.router.clone());
            }
            acc
        });

        let mut by_team = HashMap::new();
        by_team.insert(
            team_id.to_string(),
            TeamStats {
                total_requests: team_records.len() as u64,
                total_tokens: team_records.iter().map(Self::total_tokens).sum(),
                routers_used,
            },
        );

        Ok(self.aggregate_records(&team_records, Some(by_team)))
    }

    pub fn query_all_teams_usage(
        &self,
        teams: &[(&str, Vec<String>)],
        query: &UsageQuery,
        include_unknown_team: bool,
    ) -> Result<UsageStats> {
        let (start_time, end_time) = self.normalized_bounds(query);
        let records = self.db.get_usage_records_for_analytics(
            None,
            query.router.as_deref(),
            query.channel.as_deref(),
            query.model.as_deref(),
            query.status.as_deref(),
            start_time.as_deref(),
            end_time.as_deref(),
        )?;

        if records.is_empty() {
            return Ok(Self::empty_stats(Some(HashMap::new())));
        }

        let team_router_map: HashMap<&str, Vec<String>> = teams
            .iter()
            .map(|(id, routers)| (*id, routers.clone()))
            .collect();

        let mut by_team = HashMap::new();
        let mut included_records = Vec::new();

        for record in records {
            let assigned_team = if !record.team_id.is_empty()
                && team_router_map.contains_key(record.team_id.as_str())
            {
                Some(record.team_id.clone())
            } else {
                let matches = teams
                    .iter()
                    .filter(|(_, routers)| routers.iter().any(|router| router == &record.router))
                    .map(|(team_id, _)| (*team_id).to_string())
                    .collect::<Vec<_>>();

                if matches.len() == 1 {
                    Some(matches[0].clone())
                } else {
                    None
                }
            };

            let team_key = assigned_team.unwrap_or_else(|| "unknown".to_string());
            if team_key == "unknown" && !include_unknown_team {
                continue;
            }
            let token_count = Self::total_tokens(&record);

            let entry = by_team.entry(team_key).or_insert(TeamStats {
                total_requests: 0,
                total_tokens: 0,
                routers_used: Vec::new(),
            });
            entry.total_requests += 1;
            entry.total_tokens += token_count;
            if !entry
                .routers_used
                .iter()
                .any(|router| router == &record.router)
            {
                entry.routers_used.push(record.router.clone());
            }

            included_records.push(record);
        }

        Ok(self.aggregate_records(&included_records, Some(by_team)))
    }

    pub fn export_json(&self, query: &UsageQuery) -> Result<String> {
        let records = self.query_usage(query)?;
        serde_json::to_string_pretty(&records)
            .map_err(|e| anyhow::anyhow!("JSON export error: {}", e))
    }

    pub fn export_csv(&self, query: &UsageQuery) -> Result<String> {
        let records = self.query_usage(query)?;

        let mut wtr = csv::Writer::from_writer(vec![]);
        wtr.write_record([
            "timestamp",
            "team_id",
            "router",
            "channel",
            "model",
            "input_tokens",
            "output_tokens",
            "latency_ms",
            "fallback_triggered",
            "status",
        ])?;

        for record in records {
            wtr.write_record([
                &record.timestamp,
                &record.team_id,
                &record.router,
                &record.channel,
                &record.model,
                &record.input_tokens.to_string(),
                &record.output_tokens.to_string(),
                &record.latency_ms.map(|v| v.to_string()).unwrap_or_default(),
                &record.fallback_triggered.to_string(),
                &record.status,
            ])?;
        }

        let data = wtr
            .into_inner()
            .map_err(|e| anyhow::anyhow!("CSV export error: {}", e))?;
        String::from_utf8(data).map_err(|e| anyhow::anyhow!("UTF-8 error: {}", e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn create_test_engine() -> (AnalyticsEngine, TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let db = Arc::new(Database::new(Some(dir.path().to_string_lossy().to_string())).unwrap());

        db.log_usage(
            Some("req-1"),
            "team1",
            "router1",
            "channel1",
            "gpt-4",
            100,
            200,
            Some(150.0),
            false,
            "success",
            Some(200),
            None,
            None,
            None,
        );
        db.log_usage(
            Some("req-2"),
            "team1",
            "router1",
            "channel1",
            "gpt-4",
            150,
            300,
            Some(250.0),
            true,
            "fallback",
            Some(200),
            None,
            None,
            None,
        );
        db.log_usage(
            Some("req-3"),
            "team2",
            "router2",
            "channel2",
            "claude-3",
            200,
            400,
            Some(350.0),
            false,
            "error",
            Some(500),
            Some("boom"),
            None,
            None,
        );

        (AnalyticsEngine::new(db), dir)
    }

    #[test]
    fn test_query_all() {
        let (engine, _dir) = create_test_engine();
        let query = UsageQuery::default();

        let records = engine.query_usage(&query).unwrap();
        assert_eq!(records.len(), 3);
    }

    #[test]
    fn test_query_by_router() {
        let (engine, _dir) = create_test_engine();
        let query = UsageQuery {
            router: Some("router1".to_string()),
            ..Default::default()
        };

        let records = engine.query_usage(&query).unwrap();
        assert_eq!(records.len(), 2);
    }

    #[test]
    fn test_query_by_model() {
        let (engine, _dir) = create_test_engine();
        let query = UsageQuery {
            model: Some("gpt-4".to_string()),
            ..Default::default()
        };

        let records = engine.query_usage(&query).unwrap();
        assert_eq!(records.len(), 2);
    }

    #[test]
    fn test_stats_include_latency_and_errors() {
        let (engine, _dir) = create_test_engine();
        let query = UsageQuery::default();

        let stats = engine.get_stats(&query).unwrap();
        assert_eq!(stats.total_requests, 3);
        assert_eq!(stats.total_input_tokens, 450);
        assert_eq!(stats.total_output_tokens, 900);
        assert_eq!(stats.by_channel.len(), 2);
        assert_eq!(stats.error_rate, Some((1.0 / 3.0) * 100.0));
        assert_eq!(stats.p95_latency_ms, Some(250.0));
    }

    #[test]
    fn test_export_json() {
        let (engine, _dir) = create_test_engine();
        let query = UsageQuery::default();

        let json = engine.export_json(&query).unwrap();
        assert!(json.contains("team1"));
        assert!(json.contains("fallback"));
    }

    #[test]
    fn test_export_csv() {
        let (engine, _dir) = create_test_engine();
        let query = UsageQuery::default();

        let csv = engine.export_csv(&query).unwrap();
        assert!(csv.contains("timestamp,team_id,router,channel,model,input_tokens,output_tokens,latency_ms,fallback_triggered,status"));
        assert!(csv.contains("team1"));
    }

    #[test]
    fn test_query_all_teams_usage() {
        let (engine, _dir) = create_test_engine();
        let teams = vec![
            ("team1", vec!["router1".to_string()]),
            ("team2", vec!["router2".to_string()]),
        ];
        let query = UsageQuery::default();

        let stats = engine.query_all_teams_usage(&teams, &query, true).unwrap();
        let by_team = stats.by_team.unwrap();

        assert_eq!(stats.total_requests, 3);
        assert_eq!(by_team["team1"].total_requests, 2);
        assert_eq!(by_team["team2"].total_requests, 1);
    }
}
