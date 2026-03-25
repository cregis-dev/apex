use anyhow::Result;
use rusqlite::{Connection, params};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;
use tracing::info;

pub struct Database {
    conn: Mutex<Connection>,
}

#[derive(Debug, Clone, Default)]
pub struct UsageRecordQuery {
    pub team_id: Option<String>,
    pub router: Option<String>,
    pub channel: Option<String>,
    pub model: Option<String>,
    pub status: Option<String>,
    pub start_time: Option<String>,
    pub end_time: Option<String>,
}

impl Database {
    pub fn new(data_dir: Option<String>) -> Result<Self> {
        let dir = if let Some(d) = data_dir {
            if d.starts_with("~")
                && let Some(home) = dirs::home_dir()
            {
                if d == "~" {
                    return Self::new_db(home);
                }
                if let Some(stripped) = d.strip_prefix("~/") {
                    return Self::new_db(home.join(stripped));
                }
            }
            PathBuf::from(d)
        } else {
            PathBuf::from("data")
        };

        Self::new_db(dir)
    }

    fn new_db(dir: PathBuf) -> Result<Self> {
        if !dir.exists() {
            std::fs::create_dir_all(&dir)?;
        }

        let db_path = dir.join("apex.db");
        info!("Database initialized at: {:?}", db_path);

        let conn = Connection::open(&db_path)?;

        // Create tables
        conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS usage_records (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                timestamp TEXT NOT NULL,
                request_id TEXT,
                team_id TEXT NOT NULL,
                router TEXT NOT NULL,
                matched_rule TEXT,
                channel TEXT NOT NULL,
                model TEXT NOT NULL,
                input_tokens INTEGER NOT NULL DEFAULT 0,
                output_tokens INTEGER NOT NULL DEFAULT 0,
                latency_ms REAL,
                fallback_triggered INTEGER NOT NULL DEFAULT 0,
                status TEXT NOT NULL DEFAULT 'success',
                status_code INTEGER,
                error_message TEXT,
                provider_trace_id TEXT,
                provider_error_body TEXT
            );

            CREATE INDEX IF NOT EXISTS idx_usage_timestamp ON usage_records(timestamp);
            CREATE INDEX IF NOT EXISTS idx_usage_team ON usage_records(team_id);
            CREATE INDEX IF NOT EXISTS idx_usage_router ON usage_records(router);
            CREATE INDEX IF NOT EXISTS idx_usage_channel ON usage_records(channel);
            CREATE INDEX IF NOT EXISTS idx_usage_model ON usage_records(model);

            CREATE TABLE IF NOT EXISTS metrics_requests (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                timestamp TEXT NOT NULL,
                route TEXT NOT NULL,
                router TEXT NOT NULL,
                count INTEGER NOT NULL DEFAULT 1
            );

            CREATE TABLE IF NOT EXISTS metrics_errors (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                timestamp TEXT NOT NULL,
                route TEXT NOT NULL,
                router TEXT NOT NULL,
                count INTEGER NOT NULL DEFAULT 1
            );

            CREATE TABLE IF NOT EXISTS metrics_fallbacks (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                timestamp TEXT NOT NULL,
                router TEXT NOT NULL,
                channel TEXT NOT NULL,
                count INTEGER NOT NULL DEFAULT 1
            );

            CREATE TABLE IF NOT EXISTS metrics_latency (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                timestamp TEXT NOT NULL,
                route TEXT NOT NULL,
                router TEXT NOT NULL,
                channel TEXT NOT NULL,
                latency_ms REAL NOT NULL
            );

            CREATE TABLE IF NOT EXISTS gemini_replay_turns (
                cache_key TEXT PRIMARY KEY,
                team_id TEXT NOT NULL,
                model TEXT NOT NULL,
                tool_use_id TEXT NOT NULL,
                assistant_content_json TEXT NOT NULL,
                prior_messages_json TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                last_accessed_at INTEGER NOT NULL,
                expires_at INTEGER NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_metrics_timestamp ON metrics_requests(timestamp);
            CREATE INDEX IF NOT EXISTS idx_gemini_replay_expires_at ON gemini_replay_turns(expires_at);
            ",
        )?;

        let _ = conn.execute("ALTER TABLE usage_records ADD COLUMN request_id TEXT", []);
        let _ = conn.execute("ALTER TABLE usage_records ADD COLUMN matched_rule TEXT", []);
        let _ = conn.execute("ALTER TABLE usage_records ADD COLUMN latency_ms REAL", []);
        let _ = conn.execute(
            "ALTER TABLE usage_records ADD COLUMN fallback_triggered INTEGER NOT NULL DEFAULT 0",
            [],
        );
        let _ = conn.execute(
            "ALTER TABLE usage_records ADD COLUMN status TEXT NOT NULL DEFAULT 'success'",
            [],
        );
        let _ = conn.execute(
            "ALTER TABLE usage_records ADD COLUMN status_code INTEGER",
            [],
        );
        let _ = conn.execute(
            "ALTER TABLE usage_records ADD COLUMN error_message TEXT",
            [],
        );
        let _ = conn.execute(
            "ALTER TABLE usage_records ADD COLUMN provider_trace_id TEXT",
            [],
        );
        let _ = conn.execute(
            "ALTER TABLE usage_records ADD COLUMN provider_error_body TEXT",
            [],
        );

        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub fn log_usage(
        &self,
        request_id: Option<&str>,
        team_id: &str,
        router: &str,
        matched_rule: Option<&str>,
        channel: &str,
        model: &str,
        input_tokens: i64,
        output_tokens: i64,
        latency_ms: Option<f64>,
        fallback_triggered: bool,
        status: &str,
        status_code: Option<i64>,
        error_message: Option<&str>,
        provider_trace_id: Option<&str>,
        provider_error_body: Option<&str>,
    ) {
        let timestamp = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
        let model_lower = model.to_lowercase();

        if let Ok(conn) = self.conn.lock() {
            let _ = conn.execute(
                "INSERT INTO usage_records (timestamp, request_id, team_id, router, matched_rule, channel, model, input_tokens, output_tokens, latency_ms, fallback_triggered, status, status_code, error_message, provider_trace_id, provider_error_body)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
                params![
                    timestamp,
                    request_id,
                    team_id,
                    router,
                    matched_rule,
                    channel,
                    model_lower,
                    input_tokens,
                    output_tokens,
                    latency_ms,
                    if fallback_triggered { 1 } else { 0 },
                    status,
                    status_code,
                    error_message,
                    provider_trace_id,
                    provider_error_body,
                ],
            );
        }
    }

    pub fn log_request(&self, route: &str, router: &str) {
        let timestamp = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();

        if let Ok(conn) = self.conn.lock() {
            let _ = conn.execute(
                "INSERT INTO metrics_requests (timestamp, route, router) VALUES (?1, ?2, ?3)",
                params![timestamp, route, router],
            );
        }
    }

    pub fn log_error(&self, route: &str, router: &str) {
        let timestamp = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();

        if let Ok(conn) = self.conn.lock() {
            let _ = conn.execute(
                "INSERT INTO metrics_errors (timestamp, route, router) VALUES (?1, ?2, ?3)",
                params![timestamp, route, router],
            );
        }
    }

    pub fn log_fallback(&self, router: &str, channel: &str) {
        let timestamp = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();

        if let Ok(conn) = self.conn.lock() {
            let _ = conn.execute(
                "INSERT INTO metrics_fallbacks (timestamp, router, channel) VALUES (?1, ?2, ?3)",
                params![timestamp, router, channel],
            );
        }
    }

    pub fn log_latency(&self, route: &str, router: &str, channel: &str, latency_ms: f64) {
        let timestamp = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();

        if let Ok(conn) = self.conn.lock() {
            let _ = conn.execute(
                "INSERT INTO metrics_latency (timestamp, route, router, channel, latency_ms) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![timestamp, route, router, channel, latency_ms],
            );
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn upsert_gemini_replay_turn(
        &self,
        cache_key: &str,
        team_id: &str,
        model: &str,
        tool_use_id: &str,
        assistant_content_json: &str,
        prior_messages_json: &str,
        ttl: Duration,
    ) {
        let now = chrono::Utc::now().timestamp();
        let expires_at = now.saturating_add(ttl.as_secs().min(i64::MAX as u64) as i64);

        if let Ok(conn) = self.conn.lock() {
            let _ = conn.execute(
                "INSERT INTO gemini_replay_turns (
                    cache_key, team_id, model, tool_use_id, assistant_content_json,
                    prior_messages_json, created_at, last_accessed_at, expires_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
                 ON CONFLICT(cache_key) DO UPDATE SET
                    team_id = excluded.team_id,
                    model = excluded.model,
                    tool_use_id = excluded.tool_use_id,
                    assistant_content_json = excluded.assistant_content_json,
                    prior_messages_json = excluded.prior_messages_json,
                    last_accessed_at = excluded.last_accessed_at,
                    expires_at = excluded.expires_at",
                params![
                    cache_key,
                    team_id,
                    model,
                    tool_use_id,
                    assistant_content_json,
                    prior_messages_json,
                    now,
                    now,
                    expires_at,
                ],
            );
            let _ = conn.execute(
                "DELETE FROM gemini_replay_turns WHERE expires_at <= ?1",
                params![now],
            );
        }
    }

    pub fn get_gemini_replay_turn(
        &self,
        cache_key: &str,
        ttl: Duration,
    ) -> Option<GeminiReplayTurnRecord> {
        let now = chrono::Utc::now().timestamp();
        let expires_at = now.saturating_add(ttl.as_secs().min(i64::MAX as u64) as i64);

        let conn = self.conn.lock().ok()?;
        let mut stmt = conn
            .prepare(
                "SELECT assistant_content_json, prior_messages_json
                 FROM gemini_replay_turns
                 WHERE cache_key = ?1 AND expires_at > ?2",
            )
            .ok()?;
        let row = stmt
            .query_row(params![cache_key, now], |row| {
                Ok(GeminiReplayTurnRecord {
                    assistant_content_json: row.get(0)?,
                    prior_messages_json: row.get(1)?,
                })
            })
            .ok()?;
        let _ = conn.execute(
            "UPDATE gemini_replay_turns SET last_accessed_at = ?2, expires_at = ?3 WHERE cache_key = ?1",
            params![cache_key, now, expires_at],
        );
        Some(row)
    }

    // Query methods for dashboard

    #[allow(clippy::too_many_arguments)]
    pub fn get_usage_records(
        &self,
        team_id: Option<&str>,
        router: Option<&str>,
        channel: Option<&str>,
        model: Option<&str>,
        status: Option<&str>,
        start_date: Option<&str>,
        end_date: Option<&str>,
        limit: i64,
        offset: i64,
    ) -> Result<(Vec<UsageRecord>, i64)> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("{}", e))?;
        let query = UsageRecordQuery {
            team_id: team_id.map(str::to_owned),
            router: router.map(str::to_owned),
            channel: channel.map(str::to_owned),
            model: model.map(str::to_owned),
            status: status.map(str::to_owned),
            start_time: start_date.map(str::to_owned),
            end_time: end_date.map(str::to_owned),
        };
        let (where_clause, count_params_vec) = Self::build_usage_record_filters(&query, true);

        // Get total count
        let count_sql = format!(
            "SELECT COUNT(*) FROM usage_records WHERE 1=1{}",
            where_clause
        );
        let count_params_refs: Vec<&dyn rusqlite::ToSql> =
            count_params_vec.iter().map(|p| p.as_ref()).collect();
        let total: i64 =
            conn.query_row(&count_sql, count_params_refs.as_slice(), |row| row.get(0))?;

        // Get records
        let mut sql = String::from(
            "SELECT id, timestamp, request_id, team_id, router, matched_rule, channel, model, input_tokens, output_tokens, latency_ms, fallback_triggered, status, status_code, error_message, provider_trace_id, provider_error_body FROM usage_records WHERE 1=1",
        );
        sql.push_str(&where_clause);
        sql.push_str(" ORDER BY timestamp DESC, id DESC LIMIT ? OFFSET ?");

        let mut params_vec = count_params_vec;
        params_vec.push(Box::new(limit));
        params_vec.push(Box::new(offset));
        let params_refs: Vec<&dyn rusqlite::ToSql> =
            params_vec.iter().map(|p| p.as_ref()).collect();

        let mut stmt = conn.prepare(&sql)?;
        let records = stmt
            .query_map(params_refs.as_slice(), |row| {
                let channel: String = row.get(6)?;
                Ok(UsageRecord {
                    id: row.get(0)?,
                    timestamp: row.get(1)?,
                    request_id: row.get(2)?,
                    team_id: row.get(3)?,
                    router: row.get(4)?,
                    matched_rule: row.get(5)?,
                    final_channel: channel.clone(),
                    channel,
                    model: row.get(7)?,
                    input_tokens: row.get(8)?,
                    output_tokens: row.get(9)?,
                    latency_ms: row.get(10)?,
                    fallback_triggered: row.get::<_, i64>(11)? > 0,
                    status: row.get(12)?,
                    status_code: row.get(13)?,
                    error_message: row.get(14)?,
                    provider_trace_id: row.get(15)?,
                    provider_error_body: row.get(16)?,
                })
            })?
            .filter_map(|r| r.ok())
            .collect();

        Ok((records, total))
    }

    pub fn get_usage_records_for_analytics(
        &self,
        query: &UsageRecordQuery,
    ) -> Result<Vec<UsageRecord>> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("{}", e))?;
        let (where_clause, params_vec) = Self::build_usage_record_filters(query, false);
        let mut sql = String::from(
            "SELECT id, timestamp, request_id, team_id, router, matched_rule, channel, model, input_tokens, output_tokens, latency_ms, fallback_triggered, status, status_code, error_message, provider_trace_id, provider_error_body FROM usage_records WHERE 1=1",
        );
        sql.push_str(&where_clause);

        sql.push_str(" ORDER BY timestamp DESC, id DESC");

        let params_refs: Vec<&dyn rusqlite::ToSql> =
            params_vec.iter().map(|p| p.as_ref()).collect();

        let mut stmt = conn.prepare(&sql)?;
        let records = stmt
            .query_map(params_refs.as_slice(), |row| {
                let channel: String = row.get(6)?;
                Ok(UsageRecord {
                    id: row.get(0)?,
                    timestamp: row.get(1)?,
                    request_id: row.get(2)?,
                    team_id: row.get(3)?,
                    router: row.get(4)?,
                    matched_rule: row.get(5)?,
                    final_channel: channel.clone(),
                    channel,
                    model: row.get(7)?,
                    input_tokens: row.get(8)?,
                    output_tokens: row.get(9)?,
                    latency_ms: row.get(10)?,
                    fallback_triggered: row.get::<_, i64>(11)? > 0,
                    status: row.get(12)?,
                    status_code: row.get(13)?,
                    error_message: row.get(14)?,
                    provider_trace_id: row.get(15)?,
                    provider_error_body: row.get(16)?,
                })
            })?
            .filter_map(|r| r.ok())
            .collect();

        Ok(records)
    }

    fn build_usage_record_filters(
        query: &UsageRecordQuery,
        date_only: bool,
    ) -> (String, Vec<Box<dyn rusqlite::ToSql>>) {
        let mut where_clause = String::new();
        let mut params_vec: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

        if let Some(team_id) = query.team_id.as_deref() {
            where_clause.push_str(" AND team_id = ?");
            params_vec.push(Box::new(team_id.to_string()));
        }
        if let Some(router) = query.router.as_deref() {
            where_clause.push_str(" AND router = ?");
            params_vec.push(Box::new(router.to_string()));
        }
        if let Some(channel) = query.channel.as_deref() {
            where_clause.push_str(" AND channel = ?");
            params_vec.push(Box::new(channel.to_string()));
        }
        if let Some(model) = query.model.as_deref() {
            where_clause.push_str(" AND model = ?");
            params_vec.push(Box::new(model.to_string()));
        }
        if let Some(status) = query.status.as_deref() {
            match status {
                "errors" => {
                    where_clause.push_str(" AND status IN ('error', 'fallback_error')");
                }
                "fallbacks" => {
                    where_clause.push_str(" AND fallback_triggered = 1");
                }
                _ => {
                    where_clause.push_str(" AND status = ?");
                    params_vec.push(Box::new(status.to_string()));
                }
            }
        }
        if let Some(start_time) = query.start_time.as_deref() {
            if date_only {
                where_clause.push_str(" AND date(timestamp) >= date(?)");
            } else {
                where_clause.push_str(" AND timestamp >= ?");
            }
            params_vec.push(Box::new(start_time.to_string()));
        }
        if let Some(end_time) = query.end_time.as_deref() {
            if date_only {
                where_clause.push_str(" AND date(timestamp) <= date(?)");
            } else {
                where_clause.push_str(" AND timestamp <= ?");
            }
            params_vec.push(Box::new(end_time.to_string()));
        }

        (where_clause, params_vec)
    }

    #[allow(dead_code)]
    pub fn get_usage_summary(
        &self,
        team_id: Option<&str>,
        router: Option<&str>,
        channel: Option<&str>,
    ) -> Result<UsageSummary> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("{}", e))?;

        let mut sql = String::from(
            "SELECT SUM(input_tokens), SUM(output_tokens), COUNT(*) FROM usage_records WHERE 1=1",
        );
        let mut params_vec: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

        if let Some(team_id) = team_id {
            sql.push_str(" AND team_id = ?");
            params_vec.push(Box::new(team_id.to_string()));
        }
        if let Some(router) = router {
            sql.push_str(" AND router = ?");
            params_vec.push(Box::new(router.to_string()));
        }
        if let Some(channel) = channel {
            sql.push_str(" AND channel = ?");
            params_vec.push(Box::new(channel.to_string()));
        }

        let params_refs: Vec<&dyn rusqlite::ToSql> =
            params_vec.iter().map(|p| p.as_ref()).collect();

        let mut stmt = conn.prepare(&sql)?;
        let result = stmt.query_row(params_refs.as_slice(), |row| {
            Ok(UsageSummary {
                total_input_tokens: row.get::<_, i64>(0).unwrap_or(0),
                total_output_tokens: row.get::<_, i64>(1).unwrap_or(0),
                total_requests: row.get::<_, i64>(2).unwrap_or(0),
            })
        })?;

        Ok(result)
    }

    pub fn get_metrics_summary(&self) -> Result<MetricsSummary> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("{}", e))?;

        let total_requests: i64 = conn
            .query_row("SELECT COUNT(*) FROM metrics_requests", [], |row| {
                row.get(0)
            })
            .unwrap_or(0);

        let total_errors: i64 = conn
            .query_row("SELECT COUNT(*) FROM metrics_errors", [], |row| row.get(0))
            .unwrap_or(0);

        let total_fallbacks: i64 = conn
            .query_row("SELECT COUNT(*) FROM metrics_fallbacks", [], |row| {
                row.get(0)
            })
            .unwrap_or(0);

        let avg_latency: f64 = conn
            .query_row("SELECT AVG(latency_ms) FROM metrics_latency", [], |row| {
                row.get(0)
            })
            .unwrap_or(0.0);

        let p95_latency_ms = self.percentile_latency(&conn, 0.95).unwrap_or(0.0);

        Ok(MetricsSummary {
            total_requests,
            total_errors,
            total_fallbacks,
            avg_latency_ms: avg_latency,
            error_rate: if total_requests > 0 {
                (total_errors as f64 / total_requests as f64) * 100.0
            } else {
                0.0
            },
            p95_latency_ms,
        })
    }

    pub fn get_trends(
        &self,
        period: &str,
        start_date: Option<&str>,
        end_date: Option<&str>,
    ) -> Result<Vec<TrendData>> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("{}", e))?;

        let date_format = match period {
            "weekly" => "%Y-W%W",
            "monthly" => "%Y-%m",
            _ => "%Y-%m-%d", // daily
        };

        let mut sql = format!(
            "SELECT
                requests.date as date,
                requests.requests as requests,
                COALESCE(usage.input_tokens, 0) as input_tokens,
                COALESCE(usage.output_tokens, 0) as output_tokens,
                COALESCE(errors.total_errors, 0) as total_errors,
                COALESCE(fallbacks.total_fallbacks, 0) as total_fallbacks,
                COALESCE(latency.avg_latency_ms, 0) as avg_latency_ms
             FROM (
                SELECT strftime('{fmt}', timestamp) as date, COUNT(*) as requests
                FROM metrics_requests
                WHERE 1=1",
            fmt = date_format
        );
        let mut params_vec: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

        if let Some(start) = start_date {
            sql.push_str(" AND timestamp >= ?");
            params_vec.push(Box::new(format!("{} 00:00:00", start)));
        }
        if let Some(end) = end_date {
            sql.push_str(" AND timestamp <= ?");
            params_vec.push(Box::new(format!("{} 23:59:59", end)));
        }

        sql.push_str(" GROUP BY date ) requests ");

        sql.push_str(&format!(
            "LEFT JOIN (
                SELECT strftime('{fmt}', timestamp) as date,
                       COALESCE(SUM(input_tokens), 0) as input_tokens,
                       COALESCE(SUM(output_tokens), 0) as output_tokens
                FROM usage_records
                WHERE 1=1",
            fmt = date_format
        ));
        if let Some(start) = start_date {
            sql.push_str(" AND timestamp >= ?");
            params_vec.push(Box::new(format!("{} 00:00:00", start)));
        }
        if let Some(end) = end_date {
            sql.push_str(" AND timestamp <= ?");
            params_vec.push(Box::new(format!("{} 23:59:59", end)));
        }
        sql.push_str(" GROUP BY date ) usage ON usage.date = requests.date ");

        sql.push_str(&format!(
            "LEFT JOIN (
                SELECT strftime('{fmt}', timestamp) as date, COUNT(*) as total_errors
                FROM metrics_errors
                WHERE 1=1",
            fmt = date_format
        ));
        if let Some(start) = start_date {
            sql.push_str(" AND timestamp >= ?");
            params_vec.push(Box::new(format!("{} 00:00:00", start)));
        }
        if let Some(end) = end_date {
            sql.push_str(" AND timestamp <= ?");
            params_vec.push(Box::new(format!("{} 23:59:59", end)));
        }
        sql.push_str(" GROUP BY date ) errors ON errors.date = requests.date ");

        sql.push_str(&format!(
            "LEFT JOIN (
                SELECT strftime('{fmt}', timestamp) as date, COUNT(*) as total_fallbacks
                FROM metrics_fallbacks
                WHERE 1=1",
            fmt = date_format
        ));
        if let Some(start) = start_date {
            sql.push_str(" AND timestamp >= ?");
            params_vec.push(Box::new(format!("{} 00:00:00", start)));
        }
        if let Some(end) = end_date {
            sql.push_str(" AND timestamp <= ?");
            params_vec.push(Box::new(format!("{} 23:59:59", end)));
        }
        sql.push_str(" GROUP BY date ) fallbacks ON fallbacks.date = requests.date ");

        sql.push_str(&format!(
            "LEFT JOIN (
                SELECT strftime('{fmt}', timestamp) as date, AVG(latency_ms) as avg_latency_ms
                FROM metrics_latency
                WHERE 1=1",
            fmt = date_format
        ));
        if let Some(start) = start_date {
            sql.push_str(" AND timestamp >= ?");
            params_vec.push(Box::new(format!("{} 00:00:00", start)));
        }
        if let Some(end) = end_date {
            sql.push_str(" AND timestamp <= ?");
            params_vec.push(Box::new(format!("{} 23:59:59", end)));
        }

        sql.push_str(
            " GROUP BY date ) latency ON latency.date = requests.date ORDER BY requests.date ASC",
        );

        let params_refs: Vec<&dyn rusqlite::ToSql> =
            params_vec.iter().map(|p| p.as_ref()).collect();

        let mut stmt = conn.prepare(&sql)?;
        let mut trends = stmt
            .query_map(params_refs.as_slice(), |row| {
                Ok(TrendData {
                    date: row.get(0)?,
                    requests: row.get(1)?,
                    input_tokens: row.get(2)?,
                    output_tokens: row.get(3)?,
                    total_errors: row.get(4)?,
                    total_fallbacks: row.get(5)?,
                    avg_latency_ms: row.get(6)?,
                    p95_latency_ms: 0.0,
                })
            })?
            .filter_map(|r| r.ok())
            .collect::<Vec<_>>();

        let mut latency_sql = format!(
            "SELECT strftime('{fmt}', timestamp) as date, latency_ms
             FROM metrics_latency
             WHERE 1=1",
            fmt = date_format
        );
        let mut latency_params_vec: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

        if let Some(start) = start_date {
            latency_sql.push_str(" AND timestamp >= ?");
            latency_params_vec.push(Box::new(format!("{} 00:00:00", start)));
        }
        if let Some(end) = end_date {
            latency_sql.push_str(" AND timestamp <= ?");
            latency_params_vec.push(Box::new(format!("{} 23:59:59", end)));
        }

        latency_sql.push_str(" ORDER BY date ASC, latency_ms ASC");

        let latency_params_refs: Vec<&dyn rusqlite::ToSql> =
            latency_params_vec.iter().map(|p| p.as_ref()).collect();
        let mut latency_stmt = conn.prepare(&latency_sql)?;
        let latency_rows = latency_stmt.query_map(latency_params_refs.as_slice(), |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, f64>(1)?))
        })?;

        let mut latency_by_date: BTreeMap<String, Vec<f64>> = BTreeMap::new();
        for row in latency_rows {
            let (date, latency_ms) = row?;
            latency_by_date.entry(date).or_default().push(latency_ms);
        }

        for trend in &mut trends {
            if let Some(latencies) = latency_by_date.get(&trend.date)
                && !latencies.is_empty()
            {
                let rank = ((latencies.len() - 1) as f64 * 0.95).floor() as usize;
                trend.p95_latency_ms = latencies[rank];
            }
        }

        Ok(trends)
    }

    pub fn get_rankings(&self, by: &str, limit: i64) -> Result<Vec<RankingItem>> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("{}", e))?;

        let column = match by {
            "model" => "model",
            "channel" => "channel",
            "router" => "router",
            _ => "team_id",
        };

        let sql = format!(
            "SELECT
                {} as name,
                COUNT(*) as requests,
                COALESCE(SUM(input_tokens), 0) as input_tokens,
                COALESCE(SUM(output_tokens), 0) as output_tokens,
                COUNT(*) * 100.0 / (SELECT COUNT(*) FROM usage_records) as percentage
             FROM usage_records
             GROUP BY {}
             ORDER BY requests DESC
             LIMIT ?",
            column, column
        );

        let mut stmt = conn.prepare(&sql)?;
        let rankings = stmt
            .query_map([limit], |row| {
                Ok(RankingItem {
                    name: row.get(0)?,
                    requests: row.get(1)?,
                    input_tokens: row.get(2)?,
                    output_tokens: row.get(3)?,
                    percentage: row.get(4)?,
                })
            })?
            .filter_map(|r| r.ok())
            .collect();

        Ok(rankings)
    }

    fn percentile_latency(&self, conn: &Connection, percentile: f64) -> Result<f64> {
        let total_rows: i64 =
            conn.query_row("SELECT COUNT(*) FROM metrics_latency", [], |row| row.get(0))?;

        if total_rows == 0 {
            return Ok(0.0);
        }

        let clamped = percentile.clamp(0.0, 1.0);
        let rank = ((total_rows - 1) as f64 * clamped).floor() as i64;
        let latency = conn.query_row(
            "SELECT latency_ms FROM metrics_latency ORDER BY latency_ms ASC LIMIT 1 OFFSET ?1",
            [rank],
            |row| row.get(0),
        )?;

        Ok(latency)
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct UsageRecord {
    pub id: i64,
    pub timestamp: String,
    pub request_id: Option<String>,
    pub team_id: String,
    pub router: String,
    pub matched_rule: Option<String>,
    pub final_channel: String,
    pub channel: String,
    pub model: String,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub latency_ms: Option<f64>,
    pub fallback_triggered: bool,
    pub status: String,
    pub status_code: Option<i64>,
    pub error_message: Option<String>,
    pub provider_trace_id: Option<String>,
    pub provider_error_body: Option<String>,
}

#[derive(Debug, Clone)]
pub struct GeminiReplayTurnRecord {
    pub assistant_content_json: String,
    pub prior_messages_json: String,
}

#[allow(dead_code)]
#[derive(Debug, Clone, serde::Serialize)]
pub struct UsageSummary {
    pub total_input_tokens: i64,
    pub total_output_tokens: i64,
    pub total_requests: i64,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct MetricsSummary {
    pub total_requests: i64,
    pub total_errors: i64,
    pub total_fallbacks: i64,
    pub avg_latency_ms: f64,
    pub error_rate: f64,
    pub p95_latency_ms: f64,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct TrendData {
    pub date: String,
    pub requests: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub total_errors: i64,
    pub total_fallbacks: i64,
    pub avg_latency_ms: f64,
    pub p95_latency_ms: f64,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct RankingItem {
    pub name: String,
    pub requests: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub percentage: f64,
}

#[cfg(test)]
mod tests {
    use super::Database;
    use crate::database::UsageRecordQuery;
    use rusqlite::params;
    use tempfile::tempdir;

    #[test]
    fn usage_records_are_sorted_by_latest_timestamp_first() {
        let dir = tempdir().expect("create temp dir");
        let db = Database::new(Some(dir.path().to_string_lossy().into_owned())).expect("create db");

        {
            let conn = db.conn.lock().expect("lock db");
            conn.execute(
                "INSERT INTO usage_records (timestamp, request_id, team_id, router, channel, model, input_tokens, output_tokens, latency_ms, fallback_triggered, status)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                params![
                    "2026-03-10 09:00:00",
                    "req-error",
                    "team-a",
                    "primary",
                    "chat",
                    "gpt-4o",
                    10,
                    20,
                    123.0_f64,
                    0,
                    "error"
                ],
            )
            .expect("insert older error record");

            conn.execute(
                "INSERT INTO usage_records (timestamp, request_id, team_id, router, channel, model, input_tokens, output_tokens, latency_ms, fallback_triggered, status)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                params![
                    "2026-03-11 09:00:00",
                    "req-success",
                    "team-a",
                    "primary",
                    "chat",
                    "gpt-4o",
                    10,
                    20,
                    100.0_f64,
                    0,
                    "success"
                ],
            )
            .expect("insert newer success record");
        }

        let (records, total) = db
            .get_usage_records(None, None, None, None, None, None, None, 20, 0)
            .expect("query usage records");

        assert_eq!(total, 2);
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].request_id.as_deref(), Some("req-success"));
        assert_eq!(records[1].request_id.as_deref(), Some("req-error"));
    }

    #[test]
    fn analytics_records_use_id_as_tie_breaker_for_same_timestamp() {
        let dir = tempdir().expect("create temp dir");
        let db = Database::new(Some(dir.path().to_string_lossy().into_owned())).expect("create db");

        {
            let conn = db.conn.lock().expect("lock db");
            conn.execute(
                "INSERT INTO usage_records (timestamp, request_id, team_id, router, channel, model, input_tokens, output_tokens, latency_ms, fallback_triggered, status)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                params![
                    "2026-03-11 09:00:00",
                    "req-older-id",
                    "team-a",
                    "primary",
                    "chat",
                    "gpt-4o",
                    10,
                    20,
                    100.0_f64,
                    0,
                    "success"
                ],
            )
            .expect("insert first record");

            conn.execute(
                "INSERT INTO usage_records (timestamp, request_id, team_id, router, channel, model, input_tokens, output_tokens, latency_ms, fallback_triggered, status)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                params![
                    "2026-03-11 09:00:00",
                    "req-newer-id",
                    "team-a",
                    "primary",
                    "chat",
                    "gpt-4o",
                    10,
                    20,
                    100.0_f64,
                    0,
                    "success"
                ],
            )
            .expect("insert second record");
        }

        let records = db
            .get_usage_records_for_analytics(&UsageRecordQuery::default())
            .expect("query analytics records");

        assert_eq!(records.len(), 2);
        assert_eq!(records[0].request_id.as_deref(), Some("req-newer-id"));
        assert_eq!(records[1].request_id.as_deref(), Some("req-older-id"));
    }
}
