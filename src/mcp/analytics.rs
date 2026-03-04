use anyhow::Result;
use chrono::{DateTime, NaiveDateTime};
use csv::ReaderBuilder;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::File;
use std::path::PathBuf;

/// Usage record from the CSV log
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct UsageRecord {
    pub timestamp: String,
    pub team_id: String,
    pub router: String,
    pub channel: String,
    pub model: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
}

/// Query filters for analytics
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UsageQuery {
    pub team_id: Option<String>,
    pub router: Option<String>,
    pub model: Option<String>,
    pub start_time: Option<String>,
    pub end_time: Option<String>,
}

/// Aggregated usage statistics
/// Note: error_rate and latency metrics require MetricsState integration (Prometheus).
/// Currently returns None because usage.csv doesn't contain error/latency data.
/// To enable these metrics:
///   1. Extend usage.csv to log errors and latency, OR
///   2. Integrate with MetricsState (apex_errors_total, apex_upstream_latency_ms)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageStats {
    pub total_requests: u64,
    pub total_input_tokens: u64,
    pub total_output_tokens: u64,
    pub total_tokens: u64,
    /// Average latency in milliseconds. Requires MetricsState integration.
    pub avg_latency_ms: Option<f64>,
    /// P50 latency in milliseconds. Requires MetricsState integration.
    pub p50_latency_ms: Option<f64>,
    /// P95 latency in milliseconds. Requires MetricsState integration.
    pub p95_latency_ms: Option<f64>,
    /// P99 latency in milliseconds. Requires MetricsState integration.
    pub p99_latency_ms: Option<f64>,
    /// Error rate (errors / total requests). Requires MetricsState integration.
    pub error_rate: Option<f64>,
    pub by_router: HashMap<String, RouterStats>,
    pub by_channel: HashMap<String, ChannelStats>,
    pub by_model: HashMap<String, ModelStats>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub by_team: Option<HashMap<String, TeamStats>>,
}

/// Statistics per router
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouterStats {
    pub total_requests: u64,
    pub total_tokens: u64,
}

/// Statistics per channel
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelStats {
    pub total_requests: u64,
    pub total_tokens: u64,
}

/// Statistics per model
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelStats {
    pub total_requests: u64,
    pub total_tokens: u64,
}

/// Statistics per team
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamStats {
    pub total_requests: u64,
    pub total_tokens: u64,
    pub routers_used: Vec<String>,
}

pub struct AnalyticsEngine {
    log_dir: PathBuf,
}

impl AnalyticsEngine {
    pub fn new(log_dir: Option<String>) -> Self {
        let dir = if let Some(d) = log_dir {
            if d.starts_with("~")
                && let Some(home) = dirs::home_dir()
            {
                if d == "~" {
                    home.join("logs")
                } else if let Some(stripped) = d.strip_prefix("~/") {
                    home.join(stripped)
                } else {
                    PathBuf::from(d)
                }
            } else {
                PathBuf::from(d)
            }
        } else {
            PathBuf::from("logs")
        };

        Self { log_dir: dir }
    }

    fn get_usage_file_path(&self) -> PathBuf {
        self.log_dir.join("usage.csv")
    }

    /// Parse timestamp string to NaiveDateTime
    fn parse_timestamp(&self, ts: &str) -> Option<NaiveDateTime> {
        // Try format: "2024-01-15 10:30:45"
        NaiveDateTime::parse_from_str(ts, "%Y-%m-%d %H:%M:%S")
            .or_else(|_| NaiveDateTime::parse_from_str(ts, "%Y-%m-%dT%H:%M:%S"))
            .or_else(|_| DateTime::parse_from_rfc3339(ts).map(|dt| dt.naive_utc()))
            .ok()
    }

    /// Check if a record matches the query filters
    fn matches_query(&self, record: &UsageRecord, query: &UsageQuery) -> bool {
        // Check time range
        if let Some(ref start) = query.start_time
            && let Some(rec_time) = self.parse_timestamp(&record.timestamp)
            && let Some(start_dt) = self.parse_timestamp(start)
            && rec_time < start_dt
        {
            return false;
        }

        if let Some(ref end) = query.end_time
            && let Some(rec_time) = self.parse_timestamp(&record.timestamp)
            && let Some(end_dt) = self.parse_timestamp(end)
            && rec_time > end_dt
        {
            return false;
        }

        // Check router filter
        if let Some(ref router) = query.router
            && &record.router != router
        {
            return false;
        }

        // Check model filter
        if let Some(ref model) = query.model
            && &record.model != model
        {
            return false;
        }

        true
    }

    /// Query usage data with filters (supports both old and new CSV formats)
    /// Old format: timestamp,router,channel,model,input_tokens,output_tokens
    /// New format: timestamp,team_id,router,channel,model,input_tokens,output_tokens
    pub fn query_usage(&self, query: &UsageQuery) -> Result<Vec<UsageRecord>> {
        let file_path = self.get_usage_file_path();

        if !file_path.exists() {
            return Ok(vec![]);
        }

        let file = File::open(&file_path)?;
        let mut reader = ReaderBuilder::new().has_headers(true).from_reader(file);

        let headers = match reader.headers() {
            Ok(h) => h.clone(),
            Err(_) => return Ok(vec![]),
        };

        let has_team_id = headers.iter().any(|h| h == "team_id");

        let mut records = Vec::new();

        for result in reader.records() {
            let record = result?;
            if has_team_id {
                // New format with team_id
                if let Some(r) = self.parse_record_new(&record)
                    && self.matches_query(&r, query)
                {
                    records.push(r);
                }
            } else {
                // Old format without team_id - parse manually
                if let Some(r) = self.parse_record_old(&record)
                    && self.matches_query(&r, query)
                {
                    records.push(r);
                }
            }
        }

        Ok(records)
    }

    /// Parse a record in new format (with team_id)
    fn parse_record_new(&self, record: &csv::StringRecord) -> Option<UsageRecord> {
        if record.len() < 7 {
            return None;
        }
        Some(UsageRecord {
            timestamp: record.get(0)?.to_string(),
            team_id: record.get(1)?.to_string(),
            router: record.get(2)?.to_string(),
            channel: record.get(3)?.to_string(),
            model: record.get(4)?.to_string(),
            input_tokens: record.get(5)?.parse().ok()?,
            output_tokens: record.get(6)?.parse().ok()?,
        })
    }

    /// Parse a record in old format (without team_id)
    fn parse_record_old(&self, record: &csv::StringRecord) -> Option<UsageRecord> {
        if record.len() < 6 {
            return None;
        }
        Some(UsageRecord {
            timestamp: record.get(0)?.to_string(),
            team_id: String::new(), // Default empty for old format
            router: record.get(1)?.to_string(),
            channel: record.get(2)?.to_string(),
            model: record.get(3)?.to_string(),
            input_tokens: record.get(4)?.parse().ok()?,
            output_tokens: record.get(5)?.parse().ok()?,
        })
    }

    /// Get aggregated statistics
    pub fn get_stats(&self, query: &UsageQuery) -> Result<UsageStats> {
        let records = self.query_usage(query)?;

        if records.is_empty() {
            return Ok(UsageStats {
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
                by_team: None,
            });
        }

        let total_requests = records.len() as u64;
        let total_input_tokens: u64 = records.iter().map(|r| r.input_tokens).sum();
        let total_output_tokens: u64 = records.iter().map(|r| r.output_tokens).sum();
        let total_tokens = total_input_tokens + total_output_tokens;

        // Group by router
        let mut by_router: HashMap<String, RouterStats> = HashMap::new();
        for record in &records {
            let entry = by_router
                .entry(record.router.clone())
                .or_insert(RouterStats {
                    total_requests: 0,
                    total_tokens: 0,
                });
            entry.total_requests += 1;
            entry.total_tokens += record.input_tokens + record.output_tokens;
        }

        // Group by channel
        let mut by_channel: HashMap<String, ChannelStats> = HashMap::new();
        for record in &records {
            let entry = by_channel
                .entry(record.channel.clone())
                .or_insert(ChannelStats {
                    total_requests: 0,
                    total_tokens: 0,
                });
            entry.total_requests += 1;
            entry.total_tokens += record.input_tokens + record.output_tokens;
        }

        // Group by model
        let mut by_model: HashMap<String, ModelStats> = HashMap::new();
        for record in &records {
            let entry = by_model.entry(record.model.clone()).or_insert(ModelStats {
                total_requests: 0,
                total_tokens: 0,
            });
            entry.total_requests += 1;
            entry.total_tokens += record.input_tokens + record.output_tokens;
        }

        Ok(UsageStats {
            total_requests,
            total_input_tokens,
            total_output_tokens,
            total_tokens,
            avg_latency_ms: None, // Not available in current usage.csv
            p50_latency_ms: None,
            p95_latency_ms: None,
            p99_latency_ms: None,
            error_rate: None, // Not available in current usage.csv
            by_router,
            by_channel: HashMap::new(),
            by_model,
            by_team: None, // Will be populated if team_id is provided
        })
    }

    /// Query usage for a specific team
    pub fn query_team_usage(
        &self,
        team_id: &str,
        team_routers: &[String],
        query: &UsageQuery,
    ) -> Result<UsageStats> {
        // Create a query that filters by the team's allowed routers
        let mut team_query = query.clone();
        team_query.team_id = Some(team_id.to_string());

        // If no routers specified for team, we can't match
        if team_routers.is_empty() {
            return Ok(UsageStats {
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
                by_team: None,
            });
        }

        // Get stats for each router the team has access to
        let mut all_records = Vec::new();

        // Query without router filter first, then filter in memory for team routers
        let base_query = UsageQuery {
            team_id: None,
            router: None,
            model: query.model.clone(),
            start_time: query.start_time.clone(),
            end_time: query.end_time.clone(),
        };

        let records = self.query_usage(&base_query)?;
        for record in records {
            // Match team_id exactly, OR match legacy records with empty team_id
            let team_matches = record.team_id == team_id || record.team_id.is_empty();
            if team_matches && team_routers.contains(&record.router) {
                all_records.push(record);
            }
        }

        if all_records.is_empty() {
            return Ok(UsageStats {
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
                by_team: Some(HashMap::new()),
            });
        }

        let total_requests = all_records.len() as u64;
        let total_input_tokens: u64 = all_records.iter().map(|r| r.input_tokens).sum();
        let total_output_tokens: u64 = all_records.iter().map(|r| r.output_tokens).sum();
        let total_tokens = total_input_tokens + total_output_tokens;

        // Group by router
        let mut by_router: HashMap<String, RouterStats> = HashMap::new();
        let mut routers_used = Vec::new();
        for record in &all_records {
            if !routers_used.contains(&record.router) {
                routers_used.push(record.router.clone());
            }
            let entry = by_router
                .entry(record.router.clone())
                .or_insert(RouterStats {
                    total_requests: 0,
                    total_tokens: 0,
                });
            entry.total_requests += 1;
            entry.total_tokens += record.input_tokens + record.output_tokens;
        }

        // Group by channel
        let mut by_channel: HashMap<String, ChannelStats> = HashMap::new();
        for record in &all_records {
            let entry = by_channel
                .entry(record.channel.clone())
                .or_insert(ChannelStats {
                    total_requests: 0,
                    total_tokens: 0,
                });
            entry.total_requests += 1;
            entry.total_tokens += record.input_tokens + record.output_tokens;
        }

        // Group by model
        let mut by_model: HashMap<String, ModelStats> = HashMap::new();
        for record in &all_records {
            let entry = by_model.entry(record.model.clone()).or_insert(ModelStats {
                total_requests: 0,
                total_tokens: 0,
            });
            entry.total_requests += 1;
            entry.total_tokens += record.input_tokens + record.output_tokens;
        }

        // Team stats
        let mut by_team: HashMap<String, TeamStats> = HashMap::new();
        by_team.insert(
            team_id.to_string(),
            TeamStats {
                total_requests,
                total_tokens,
                routers_used,
            },
        );

        Ok(UsageStats {
            total_requests,
            total_input_tokens,
            total_output_tokens,
            total_tokens,
            avg_latency_ms: None,
            p50_latency_ms: None,
            p95_latency_ms: None,
            p99_latency_ms: None,
            error_rate: None,
            by_router,
            by_channel,
            by_model,
            by_team: Some(by_team),
        })
    }

    /// Query usage statistics for all teams
    /// Takes a list of teams with their allowed routers and returns aggregated stats per team
    pub fn query_all_teams_usage(
        &self,
        teams: &[(&str, Vec<String>)], // (team_id, allowed_routers)
        query: &UsageQuery,
    ) -> Result<UsageStats> {
        let base_query = UsageQuery {
            team_id: None,
            router: None,
            model: query.model.clone(),
            start_time: query.start_time.clone(),
            end_time: query.end_time.clone(),
        };

        // Get all records
        let all_records = self.query_usage(&base_query)?;

        if all_records.is_empty() {
            return Ok(UsageStats {
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
                by_team: Some(HashMap::new()),
            });
        }

        let mut total_requests = 0u64;
        let mut total_input_tokens = 0u64;
        let mut total_output_tokens = 0u64;
        let mut by_team: HashMap<String, TeamStats> = HashMap::new();

        // Collect all known team IDs
        let known_teams: std::collections::HashSet<&str> =
            teams.iter().map(|(id, _)| *id).collect();

        // For each team, filter records by their allowed routers
        for (team_id, team_routers) in teams {
            if team_routers.is_empty() {
                continue;
            }

            let team_records: Vec<&UsageRecord> = all_records
                .iter()
                .filter(|r| r.team_id == *team_id && team_routers.contains(&r.router))
                .collect();

            if team_records.is_empty() {
                continue;
            }

            let team_req_count = team_records.len() as u64;
            let team_input: u64 = team_records.iter().map(|r| r.input_tokens).sum();
            let team_output: u64 = team_records.iter().map(|r| r.output_tokens).sum();

            total_requests += team_req_count;
            total_input_tokens += team_input;
            total_output_tokens += team_output;

            // Get unique routers used by this team
            let routers_used: Vec<String> = team_records
                .iter()
                .map(|r| r.router.clone())
                .collect::<std::collections::HashSet<_>>()
                .into_iter()
                .collect();

            by_team.insert(
                team_id.to_string(),
                TeamStats {
                    total_requests: team_req_count,
                    total_tokens: team_input + team_output,
                    routers_used,
                },
            );
        }

        // Handle records with unknown/empty team_id (legacy records without team_id)
        let unknown_records: Vec<&UsageRecord> = all_records
            .iter()
            .filter(|r| r.team_id.is_empty() || !known_teams.contains(r.team_id.as_str()))
            .collect();

        if !unknown_records.is_empty() {
            let unknown_req_count = unknown_records.len() as u64;
            let unknown_input: u64 = unknown_records.iter().map(|r| r.input_tokens).sum();
            let unknown_output: u64 = unknown_records.iter().map(|r| r.output_tokens).sum();

            total_requests += unknown_req_count;
            total_input_tokens += unknown_input;
            total_output_tokens += unknown_output;

            let routers_used: Vec<String> = unknown_records
                .iter()
                .map(|r| r.router.clone())
                .collect::<std::collections::HashSet<_>>()
                .into_iter()
                .collect();

            by_team.insert(
                "unknown".to_string(),
                TeamStats {
                    total_requests: unknown_req_count,
                    total_tokens: unknown_input + unknown_output,
                    routers_used,
                },
            );
        }

        let total_tokens = total_input_tokens + total_output_tokens;

        // Group by router
        let mut by_router: HashMap<String, RouterStats> = HashMap::new();
        for record in &all_records {
            let entry = by_router
                .entry(record.router.clone())
                .or_insert(RouterStats {
                    total_requests: 0,
                    total_tokens: 0,
                });
            entry.total_requests += 1;
            entry.total_tokens += record.input_tokens + record.output_tokens;
        }

        // Group by channel
        let mut by_channel: HashMap<String, ChannelStats> = HashMap::new();
        for record in &all_records {
            let entry = by_channel
                .entry(record.channel.clone())
                .or_insert(ChannelStats {
                    total_requests: 0,
                    total_tokens: 0,
                });
            entry.total_requests += 1;
            entry.total_tokens += record.input_tokens + record.output_tokens;
        }

        // Group by model
        let mut by_model: HashMap<String, ModelStats> = HashMap::new();
        for record in &all_records {
            let entry = by_model.entry(record.model.clone()).or_insert(ModelStats {
                total_requests: 0,
                total_tokens: 0,
            });
            entry.total_requests += 1;
            entry.total_tokens += record.input_tokens + record.output_tokens;
        }

        Ok(UsageStats {
            total_requests,
            total_input_tokens,
            total_output_tokens,
            total_tokens,
            avg_latency_ms: None,
            p50_latency_ms: None,
            p95_latency_ms: None,
            p99_latency_ms: None,
            error_rate: None,
            by_router,
            by_channel,
            by_model,
            by_team: Some(by_team),
        })
    }

    /// Export usage data to JSON
    pub fn export_json(&self, query: &UsageQuery) -> Result<String> {
        let records = self.query_usage(query)?;
        serde_json::to_string_pretty(&records)
            .map_err(|e| anyhow::anyhow!("JSON export error: {}", e))
    }

    /// Export usage data to CSV
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
    use std::fs;
    use tempfile::tempdir;

    fn create_test_csv(dir: &tempfile::TempDir) -> PathBuf {
        let path = dir.path().join("usage.csv");
        let content = "timestamp,team_id,router,channel,model,input_tokens,output_tokens\n\
            2024-01-15 10:00:00,team1,router1,channel1,gpt-4,100,200\n\
            2024-01-15 10:05:00,team1,router1,channel1,gpt-4,150,300\n\
            2024-01-15 10:10:00,team2,router2,channel2,claude-3,200,400\n";
        fs::write(&path, content).unwrap();
        path
    }

    #[test]
    fn test_query_all() {
        let dir = tempdir().unwrap();
        create_test_csv(&dir);

        let engine = AnalyticsEngine::new(Some(dir.path().to_str().unwrap().to_string()));
        let query = UsageQuery::default();

        let records = engine.query_usage(&query).unwrap();
        assert_eq!(records.len(), 3);
    }

    #[test]
    fn test_query_by_router() {
        let dir = tempdir().unwrap();
        create_test_csv(&dir);

        let engine = AnalyticsEngine::new(Some(dir.path().to_str().unwrap().to_string()));
        let query = UsageQuery {
            router: Some("router1".to_string()),
            ..Default::default()
        };

        let records = engine.query_usage(&query).unwrap();
        assert_eq!(records.len(), 2);
    }

    #[test]
    fn test_query_by_model() {
        let dir = tempdir().unwrap();
        create_test_csv(&dir);

        let engine = AnalyticsEngine::new(Some(dir.path().to_str().unwrap().to_string()));
        let query = UsageQuery {
            model: Some("gpt-4".to_string()),
            ..Default::default()
        };

        let records = engine.query_usage(&query).unwrap();
        assert_eq!(records.len(), 2);
    }

    #[test]
    fn test_stats() {
        let dir = tempdir().unwrap();
        create_test_csv(&dir);

        let engine = AnalyticsEngine::new(Some(dir.path().to_str().unwrap().to_string()));
        let query = UsageQuery::default();

        let stats = engine.get_stats(&query).unwrap();
        assert_eq!(stats.total_requests, 3);
        assert_eq!(stats.total_input_tokens, 450);
        assert_eq!(stats.total_output_tokens, 900);
    }

    #[test]
    fn test_export_json() {
        let dir = tempdir().unwrap();
        create_test_csv(&dir);

        let engine = AnalyticsEngine::new(Some(dir.path().to_str().unwrap().to_string()));
        let query = UsageQuery::default();

        let json = engine.export_json(&query).unwrap();
        assert!(json.contains("router1"));
    }

    #[test]
    fn test_export_csv() {
        let dir = tempdir().unwrap();
        create_test_csv(&dir);

        let engine = AnalyticsEngine::new(Some(dir.path().to_str().unwrap().to_string()));
        let query = UsageQuery::default();

        let csv = engine.export_csv(&query).unwrap();
        assert!(csv.contains("timestamp,team_id,router,channel,model,input_tokens,output_tokens"));
    }

    #[test]
    fn test_query_all_teams_usage() {
        let dir = tempdir().unwrap();
        create_test_csv(&dir);

        let engine = AnalyticsEngine::new(Some(dir.path().to_str().unwrap().to_string()));
        let query = UsageQuery::default();

        // Two teams: team1 has router1, team2 has router2
        let teams = vec![
            ("team1", vec!["router1".to_string()]),
            ("team2", vec!["router2".to_string()]),
        ];

        let stats = engine.query_all_teams_usage(&teams, &query).unwrap();

        // Should have by_team stats
        assert!(stats.by_team.is_some());
        let by_team = stats.by_team.unwrap();

        // team1 should have 2 requests (router1 records)
        let team1_stats = by_team.get("team1").unwrap();
        assert_eq!(team1_stats.total_requests, 2);
        assert!(team1_stats.routers_used.contains(&"router1".to_string()));

        // team2 should have 1 request (router2 record)
        let team2_stats = by_team.get("team2").unwrap();
        assert_eq!(team2_stats.total_requests, 1);
        assert!(team2_stats.routers_used.contains(&"router2".to_string()));
    }
}
