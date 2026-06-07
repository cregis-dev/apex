use anyhow::Result;
use rusqlite::{Connection, OptionalExtension, params};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;
use tracing::info;

pub struct Database {
    /// Single writer connection: all INSERT/DELETE and the gemini replay
    /// read-then-touch go through here.
    conn: Mutex<Connection>,
    /// Dedicated read connection for dashboard/analytics queries. In WAL mode
    /// it reads a consistent snapshot without taking the writer's lock, so a
    /// slow dashboard query no longer stalls request-path logging.
    read_conn: Mutex<Connection>,
}

#[derive(Debug, Clone, Default)]
pub struct UsageRecordQuery {
    pub team_id: Option<String>,
    pub router: Option<String>,
    pub channel: Option<String>,
    pub model: Option<String>,
    pub status: Option<String>,
    pub client: Option<String>,
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

        // Tune SQLite for a write-heavy, single-file workload:
        // - WAL + synchronous=NORMAL: each request-path INSERT no longer forces
        //   a full fsync (only at checkpoint), cutting write latency on the hot
        //   path, and readers no longer take a blocking shared lock against the
        //   rollback journal.
        // - busy_timeout: wait instead of failing fast on transient contention.
        // - auto_vacuum=INCREMENTAL: lets cleanup_old_records() hand freed pages
        //   back to the OS without a full-file VACUUM. Only takes effect on a
        //   fresh DB or after a one-time VACUUM on pre-existing files.
        // auto_vacuum must be set before the connection first reads/writes the
        // file (journal_mode=WAL below would otherwise lock it in at the default
        // NONE), so it goes first.
        conn.execute_batch(
            "PRAGMA auto_vacuum=INCREMENTAL;
             PRAGMA journal_mode=WAL;
             PRAGMA synchronous=NORMAL;
             PRAGMA busy_timeout=5000;",
        )?;

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
                provider_error_body TEXT,
                client TEXT,
                user_agent TEXT
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
            CREATE INDEX IF NOT EXISTS idx_metrics_errors_timestamp ON metrics_errors(timestamp);
            CREATE INDEX IF NOT EXISTS idx_metrics_fallbacks_timestamp ON metrics_fallbacks(timestamp);
            CREATE INDEX IF NOT EXISTS idx_metrics_latency_timestamp ON metrics_latency(timestamp);
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
        // Client/tool attribution (Claude Code, Codex, SDKs, …) from request headers.
        let _ = conn.execute("ALTER TABLE usage_records ADD COLUMN client TEXT", []);
        let _ = conn.execute("ALTER TABLE usage_records ADD COLUMN user_agent TEXT", []);
        let _ = conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_usage_client ON usage_records(client)",
            [],
        );

        // Separate read connection (WAL snapshot reads don't block the writer).
        // query_only guards against an accidental write slipping onto this path.
        let read_conn = Connection::open(&db_path)?;
        read_conn.execute_batch("PRAGMA busy_timeout=5000; PRAGMA query_only=ON;")?;

        Ok(Self {
            conn: Mutex::new(conn),
            read_conn: Mutex::new(read_conn),
        })
    }

    /// Prune usage history and request/error/fallback/latency metrics older than
    /// `retention_days`, then return the freed pages to the OS. A no-op when
    /// `retention_days == 0` (keep forever). Returns the number of rows deleted.
    ///
    /// Run this off the async runtime (e.g. via `spawn_blocking`) — it holds the
    /// connection mutex and can scan large tables.
    pub fn cleanup_old_records(&self, retention_days: u64) -> Result<u64> {
        if retention_days == 0 {
            return Ok(0);
        }
        let cutoff = (chrono::Local::now() - chrono::Duration::days(retention_days as i64))
            .format("%Y-%m-%d %H:%M:%S")
            .to_string();

        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("{}", e))?;
        let mut deleted = 0u64;
        // Table names are hardcoded constants — no SQL-injection surface.
        for table in [
            "usage_records",
            "metrics_requests",
            "metrics_errors",
            "metrics_fallbacks",
            "metrics_latency",
        ] {
            let removed = conn.execute(
                &format!("DELETE FROM {table} WHERE timestamp < ?1"),
                params![cutoff],
            )?;
            deleted += removed as u64;
        }

        // Reclaim freed pages without a full-file rewrite (needs auto_vacuum=
        // INCREMENTAL, which a one-time VACUUM activates on pre-existing DBs),
        // then truncate the WAL so it does not grow unbounded.
        let _ = conn.execute_batch("PRAGMA incremental_vacuum; PRAGMA wal_checkpoint(TRUNCATE);");

        Ok(deleted)
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
        client: Option<&str>,
        user_agent: Option<&str>,
    ) {
        let timestamp = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
        let model_lower = model.to_lowercase();

        if let Ok(conn) = self.conn.lock() {
            let _ = conn.execute(
                "INSERT INTO usage_records (timestamp, request_id, team_id, router, matched_rule, channel, model, input_tokens, output_tokens, latency_ms, fallback_triggered, status, status_code, error_message, provider_trace_id, provider_error_body, client, user_agent)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18)",
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
                    client,
                    user_agent,
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

    /// Column list for `usage_records` reads, kept in lock-step with
    /// [`Self::map_usage_record`]'s positional `row.get(N)` indices.
    const USAGE_RECORD_COLUMNS: &'static str = "id, timestamp, request_id, team_id, router, matched_rule, channel, model, input_tokens, output_tokens, latency_ms, fallback_triggered, status, status_code, error_message, provider_trace_id, provider_error_body, client, user_agent";

    /// Map one `usage_records` row (selected via [`Self::USAGE_RECORD_COLUMNS`])
    /// into a [`UsageRecord`]. Single source of truth for column ordering.
    fn map_usage_record(row: &rusqlite::Row<'_>) -> rusqlite::Result<UsageRecord> {
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
            client: row.get(17)?,
            user_agent: row.get(18)?,
        })
    }

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
        let conn = self
            .read_conn
            .lock()
            .map_err(|e| anyhow::anyhow!("{}", e))?;
        let query = UsageRecordQuery {
            team_id: team_id.map(str::to_owned),
            router: router.map(str::to_owned),
            channel: channel.map(str::to_owned),
            model: model.map(str::to_owned),
            status: status.map(str::to_owned),
            client: None,
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
        let mut sql = format!(
            "SELECT {} FROM usage_records WHERE 1=1",
            Self::USAGE_RECORD_COLUMNS
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
            .query_map(params_refs.as_slice(), Self::map_usage_record)?
            .filter_map(|r| r.ok())
            .collect();

        Ok((records, total))
    }

    /// Distinct model names previously observed for the given team in the
    /// usage log. Used by `GET /v1/models` to surface concrete model ids that
    /// the team has actually been able to call, beyond the literal model
    /// names declared in the router config.
    pub fn distinct_models_for_team(&self, team_id: &str) -> Result<Vec<String>> {
        let conn = self
            .read_conn
            .lock()
            .map_err(|e| anyhow::anyhow!("{}", e))?;
        let mut stmt = conn.prepare(
            "SELECT DISTINCT model FROM usage_records \
             WHERE team_id = ? AND model IS NOT NULL AND model != '' \
             ORDER BY model",
        )?;
        let rows = stmt
            .query_map([team_id], |row| row.get::<_, String>(0))?
            .filter_map(|r| r.ok())
            .collect();
        Ok(rows)
    }

    pub fn get_usage_records_for_analytics(
        &self,
        query: &UsageRecordQuery,
    ) -> Result<Vec<UsageRecord>> {
        let conn = self
            .read_conn
            .lock()
            .map_err(|e| anyhow::anyhow!("{}", e))?;
        let (where_clause, params_vec) = Self::build_usage_record_filters(query, false);
        let mut sql = format!(
            "SELECT {} FROM usage_records WHERE 1=1",
            Self::USAGE_RECORD_COLUMNS
        );
        sql.push_str(&where_clause);

        sql.push_str(" ORDER BY timestamp DESC, id DESC");

        let params_refs: Vec<&dyn rusqlite::ToSql> =
            params_vec.iter().map(|p| p.as_ref()).collect();

        let mut stmt = conn.prepare(&sql)?;
        let records = stmt
            .query_map(params_refs.as_slice(), Self::map_usage_record)?
            .filter_map(|r| r.ok())
            .collect();

        Ok(records)
    }

    /// Bounded page of usage records for the dashboard's records view, plus the
    /// `total`, `new_records` and `latest_cursor` it needs — all computed in
    /// SQL so polling never materializes the whole window into memory. Uses the
    /// exact-timestamp filters (not date-only), matching the analytics view.
    pub fn get_usage_records_page(
        &self,
        query: &UsageRecordQuery,
        limit: i64,
        offset: i64,
        since_timestamp: Option<&str>,
        since_id: Option<i64>,
    ) -> Result<UsageRecordPage> {
        let conn = self
            .read_conn
            .lock()
            .map_err(|e| anyhow::anyhow!("{}", e))?;
        let (where_clause, params_vec) = Self::build_usage_record_filters(query, false);
        let params_refs: Vec<&dyn rusqlite::ToSql> =
            params_vec.iter().map(|p| p.as_ref()).collect();

        // Total rows in the window.
        let total: i64 = conn.query_row(
            &format!("SELECT COUNT(*) FROM usage_records WHERE 1=1{where_clause}"),
            params_refs.as_slice(),
            |row| row.get(0),
        )?;

        // Newest row in the window → the cursor clients poll against.
        let latest_cursor = conn
            .query_row(
                &format!(
                    "SELECT id, timestamp FROM usage_records WHERE 1=1{where_clause} ORDER BY timestamp DESC, id DESC LIMIT 1"
                ),
                params_refs.as_slice(),
                |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)),
            )
            .optional()?;

        // Count of rows strictly newer than the client's last-seen cursor.
        let new_records = match (since_timestamp, since_id) {
            (Some(ts), Some(id)) => {
                let (_, mut p) = Self::build_usage_record_filters(query, false);
                p.push(Box::new(ts.to_string()));
                p.push(Box::new(ts.to_string()));
                p.push(Box::new(id));
                let refs: Vec<&dyn rusqlite::ToSql> = p.iter().map(|b| b.as_ref()).collect();
                conn.query_row(
                    &format!(
                        "SELECT COUNT(*) FROM usage_records WHERE 1=1{where_clause} AND (timestamp > ? OR (timestamp = ? AND id > ?))"
                    ),
                    refs.as_slice(),
                    |row| row.get(0),
                )?
            }
            _ => 0,
        };

        // The page itself.
        let (_, mut data_params) = Self::build_usage_record_filters(query, false);
        data_params.push(Box::new(limit));
        data_params.push(Box::new(offset));
        let data_refs: Vec<&dyn rusqlite::ToSql> = data_params.iter().map(|b| b.as_ref()).collect();
        let mut stmt = conn.prepare(&format!(
            "SELECT {} FROM usage_records WHERE 1=1{where_clause} ORDER BY timestamp DESC, id DESC LIMIT ? OFFSET ?",
            Self::USAGE_RECORD_COLUMNS
        ))?;
        let records = stmt
            .query_map(data_refs.as_slice(), Self::map_usage_record)?
            .filter_map(|r| r.ok())
            .collect();

        Ok(UsageRecordPage {
            records,
            total,
            new_records,
            latest_cursor,
        })
    }

    /// Single-query aggregate over a usage window — request count, total tokens,
    /// error count and average latency. Lets the overview's period-over-period
    /// deltas compare windows without loading every row of the previous window.
    pub fn get_usage_aggregate(&self, query: &UsageRecordQuery) -> Result<UsageAggregate> {
        let conn = self
            .read_conn
            .lock()
            .map_err(|e| anyhow::anyhow!("{}", e))?;
        let (where_clause, params_vec) = Self::build_usage_record_filters(query, false);
        let refs: Vec<&dyn rusqlite::ToSql> = params_vec.iter().map(|p| p.as_ref()).collect();
        let agg = conn.query_row(
            &format!(
                "SELECT \
                   COUNT(*), \
                   COALESCE(SUM(max(input_tokens, 0) + max(output_tokens, 0)), 0), \
                   COALESCE(SUM(CASE WHEN status IN ('error', 'fallback_error') THEN 1 ELSE 0 END), 0), \
                   COALESCE(AVG(latency_ms), 0) \
                 FROM usage_records WHERE 1=1{where_clause}"
            ),
            refs.as_slice(),
            |row| {
                Ok(UsageAggregate {
                    requests: row.get(0)?,
                    total_tokens: row.get(1)?,
                    error_count: row.get(2)?,
                    avg_latency_ms: row.get(3)?,
                })
            },
        )?;
        Ok(agg)
    }

    /// Distinct filter values present in a window, for the dashboard's filter
    /// dropdowns — `SELECT DISTINCT` per column instead of loading every row.
    pub fn get_filter_options(&self, query: &UsageRecordQuery) -> Result<FilterOptions> {
        let conn = self
            .read_conn
            .lock()
            .map_err(|e| anyhow::anyhow!("{}", e))?;
        let (where_clause, params_vec) = Self::build_usage_record_filters(query, false);

        // `column` is always a hardcoded constant below — no injection surface.
        let distinct = |column: &str, non_empty: bool| -> Result<Vec<String>> {
            let extra = if non_empty {
                format!(" AND {column} IS NOT NULL AND {column} != ''")
            } else {
                String::new()
            };
            let refs: Vec<&dyn rusqlite::ToSql> = params_vec.iter().map(|p| p.as_ref()).collect();
            let mut stmt = conn.prepare(&format!(
                "SELECT DISTINCT {column} FROM usage_records WHERE 1=1{where_clause}{extra} ORDER BY {column}"
            ))?;
            let rows = stmt
                .query_map(refs.as_slice(), |row| row.get::<_, String>(0))?
                .filter_map(|r| r.ok())
                .collect();
            Ok(rows)
        };

        Ok(FilterOptions {
            teams: distinct("team_id", false)?,
            models: distinct("model", false)?,
            routers: distinct("router", false)?,
            channels: distinct("channel", false)?,
            clients: distinct("client", true)?,
        })
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
        if let Some(client) = query.client.as_deref() {
            where_clause.push_str(" AND client = ?");
            params_vec.push(Box::new(client.to_string()));
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
        let conn = self
            .read_conn
            .lock()
            .map_err(|e| anyhow::anyhow!("{}", e))?;

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
        let conn = self
            .read_conn
            .lock()
            .map_err(|e| anyhow::anyhow!("{}", e))?;

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
        let conn = self
            .read_conn
            .lock()
            .map_err(|e| anyhow::anyhow!("{}", e))?;

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
        let conn = self
            .read_conn
            .lock()
            .map_err(|e| anyhow::anyhow!("{}", e))?;

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
    pub client: Option<String>,
    pub user_agent: Option<String>,
}

/// A bounded page of usage records plus the counts the dashboard records view
/// needs, computed in SQL by [`Database::get_usage_records_page`].
pub struct UsageRecordPage {
    pub records: Vec<UsageRecord>,
    pub total: i64,
    pub new_records: i64,
    /// `(id, timestamp)` of the newest row in the window, if any.
    pub latest_cursor: Option<(i64, String)>,
}

/// Window aggregate from [`Database::get_usage_aggregate`].
pub struct UsageAggregate {
    pub requests: i64,
    pub total_tokens: i64,
    pub error_count: i64,
    pub avg_latency_ms: f64,
}

/// Distinct filter values from [`Database::get_filter_options`].
pub struct FilterOptions {
    pub teams: Vec<String>,
    pub models: Vec<String>,
    pub routers: Vec<String>,
    pub channels: Vec<String>,
    pub clients: Vec<String>,
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
    fn pragmas_enable_wal_and_incremental_autovacuum() {
        let dir = tempdir().expect("create temp dir");
        let db = Database::new(Some(dir.path().to_string_lossy().into_owned())).expect("create db");
        let conn = db.conn.lock().expect("lock db");

        let journal_mode: String = conn
            .query_row("PRAGMA journal_mode", [], |row| row.get(0))
            .expect("read journal_mode");
        assert_eq!(journal_mode.to_lowercase(), "wal");

        // auto_vacuum: 0=NONE, 1=FULL, 2=INCREMENTAL
        let auto_vacuum: i64 = conn
            .query_row("PRAGMA auto_vacuum", [], |row| row.get(0))
            .expect("read auto_vacuum");
        assert_eq!(auto_vacuum, 2);
    }

    #[test]
    fn cleanup_old_records_prunes_only_rows_past_retention() {
        let dir = tempdir().expect("create temp dir");
        let db = Database::new(Some(dir.path().to_string_lossy().into_owned())).expect("create db");

        let old_ts = (chrono::Local::now() - chrono::Duration::days(120))
            .format("%Y-%m-%d %H:%M:%S")
            .to_string();
        let recent_ts = (chrono::Local::now() - chrono::Duration::days(10))
            .format("%Y-%m-%d %H:%M:%S")
            .to_string();

        {
            let conn = db.conn.lock().expect("lock db");
            for ts in [&old_ts, &recent_ts] {
                conn.execute(
                    "INSERT INTO usage_records (timestamp, team_id, router, channel, model)
                     VALUES (?1, 'team-a', 'primary', 'chat', 'gpt-4o')",
                    params![ts],
                )
                .expect("insert usage record");
                conn.execute(
                    "INSERT INTO metrics_latency (timestamp, route, router, channel, latency_ms)
                     VALUES (?1, '/v1/messages', 'primary', 'chat', 100.0)",
                    params![ts],
                )
                .expect("insert latency record");
            }
        }

        // 0 disables pruning — nothing removed.
        assert_eq!(db.cleanup_old_records(0).expect("noop cleanup"), 0);

        // 90-day retention drops the 120-day-old rows (usage + latency = 2),
        // keeps the 10-day-old ones.
        let deleted = db.cleanup_old_records(90).expect("cleanup");
        assert_eq!(deleted, 2);

        let conn = db.conn.lock().expect("lock db");
        let usage_left: i64 = conn
            .query_row("SELECT COUNT(*) FROM usage_records", [], |row| row.get(0))
            .expect("count usage");
        let latency_left: i64 = conn
            .query_row("SELECT COUNT(*) FROM metrics_latency", [], |row| row.get(0))
            .expect("count latency");
        assert_eq!(usage_left, 1);
        assert_eq!(latency_left, 1);
    }

    #[test]
    fn usage_records_page_paginates_and_counts_in_sql() {
        let dir = tempdir().expect("create temp dir");
        let db = Database::new(Some(dir.path().to_string_lossy().into_owned())).expect("create db");

        {
            let conn = db.conn.lock().expect("lock db");
            // Ascending timestamps → ids 1..=5, newest is id 5.
            for i in 1..=5 {
                conn.execute(
                    "INSERT INTO usage_records (timestamp, team_id, router, channel, model, input_tokens, output_tokens)
                     VALUES (?1, 'team-a', 'primary', 'chat', 'gpt-4o', 1, 1)",
                    params![format!("2026-06-01 10:00:0{i}")],
                )
                .expect("insert usage record");
            }
        }

        let q = UsageRecordQuery::default();

        // First page: newest first, exact ordering.
        let page = db
            .get_usage_records_page(&q, 2, 0, None, None)
            .expect("page 1");
        assert_eq!(page.total, 5);
        assert_eq!(page.new_records, 0); // no cursor supplied
        assert_eq!(
            page.latest_cursor,
            Some((5, "2026-06-01 10:00:05".to_string()))
        );
        assert_eq!(
            page.records.iter().map(|r| r.id).collect::<Vec<_>>(),
            vec![5, 4]
        );

        // Second page via OFFSET.
        let page2 = db
            .get_usage_records_page(&q, 2, 2, None, None)
            .expect("page 2");
        assert_eq!(
            page2.records.iter().map(|r| r.id).collect::<Vec<_>>(),
            vec![3, 2]
        );

        // new_records: rows strictly newer than the (t2, id2) cursor → 3, 4, 5.
        let polled = db
            .get_usage_records_page(&q, 2, 0, Some("2026-06-01 10:00:02"), Some(2))
            .expect("poll");
        assert_eq!(polled.new_records, 3);

        // Caught up to the newest cursor → nothing new.
        let caught_up = db
            .get_usage_records_page(&q, 2, 0, Some("2026-06-01 10:00:05"), Some(5))
            .expect("poll caught up");
        assert_eq!(caught_up.new_records, 0);
    }

    #[test]
    fn usage_aggregate_and_filter_options_match_in_sql() {
        let dir = tempdir().expect("create temp dir");
        let db = Database::new(Some(dir.path().to_string_lossy().into_owned())).expect("create db");

        {
            let conn = db.conn.lock().expect("lock db");
            let insert = |team: &str,
                          model: &str,
                          input: i64,
                          output: i64,
                          status: &str,
                          latency: Option<f64>,
                          client: Option<&str>| {
                conn.execute(
                    "INSERT INTO usage_records (timestamp, team_id, router, channel, model, input_tokens, output_tokens, latency_ms, status, client)
                     VALUES ('2026-06-01 10:00:00', ?1, 'primary', 'chat', ?2, ?3, ?4, ?5, ?6, ?7)",
                    params![team, model, input, output, latency, status, client],
                )
                .expect("insert usage record");
            };
            insert(
                "team-a",
                "gpt-4o",
                10,
                5,
                "success",
                Some(100.0),
                Some("claude-code"),
            );
            insert("team-b", "gpt-4o", 20, 0, "error", Some(200.0), Some(""));
            insert("team-a", "kimi", 0, 0, "fallback_error", None, None);
        }

        let agg = db
            .get_usage_aggregate(&UsageRecordQuery::default())
            .expect("aggregate");
        assert_eq!(agg.requests, 3);
        assert_eq!(agg.total_tokens, 35); // (10+5)+(20+0)+(0+0)
        assert_eq!(agg.error_count, 2); // error + fallback_error
        assert!((agg.avg_latency_ms - 150.0).abs() < f64::EPSILON); // NULL latency excluded

        let opts = db
            .get_filter_options(&UsageRecordQuery::default())
            .expect("filter options");
        assert_eq!(opts.teams, vec!["team-a", "team-b"]); // distinct, sorted
        assert_eq!(opts.models, vec!["gpt-4o", "kimi"]);
        assert_eq!(opts.channels, vec!["chat"]);
        // empty / NULL clients are excluded; only the real one remains.
        assert_eq!(opts.clients, vec!["claude-code"]);
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
