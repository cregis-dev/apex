export type DashboardRange = "1h" | "24h" | "7d" | "30d";

export type DashboardTab = "overview" | "team" | "system" | "model" | "records";

export interface DashboardFilterOptions {
  teams: string[];
  models: string[];
  routers: string[];
  channels: string[];
}

export interface DashboardOverview {
  total_requests: number;
  total_tokens: number;
  input_tokens: number;
  output_tokens: number;
  avg_latency_ms: number;
  success_rate: number;
  delta: {
    total_requests: number;
    total_tokens: number;
    avg_latency_ms: number;
    success_rate: number;
  };
}

export interface DashboardTrendPoint {
  bucket: string;
  label: string;
  requests: number;
  input_tokens: number;
  output_tokens: number;
  total_tokens: number;
  error_rate: number;
  avg_latency_ms: number;
  success_rate: number;
}

export interface DashboardTrendSection {
  unit: string;
  points: DashboardTrendPoint[];
}

export interface DashboardTeamLeaderboardItem {
  team_id: string;
  total_requests: number;
  total_tokens: number;
}

export interface DashboardTeamModelUsageItem {
  team_id: string;
  model: string;
  total_requests: number;
  total_tokens: number;
}

export interface DashboardChannelLatencyItem {
  channel: string;
  total_requests: number;
  avg_latency_ms: number;
  p95_latency_ms: number;
}

export interface DashboardShareItem {
  name: string;
  requests: number;
  total_tokens: number;
  percentage: number;
}

export interface DashboardTopologyNode {
  name: string;
  kind: string;
}

export interface DashboardTopologyLink {
  source: number;
  target: number;
  value: number;
}

export interface DashboardFlowSummary {
  team_id: string;
  router: string;
  channel: string;
  model: string;
  requests: number;
  total_tokens: number;
}

export interface DashboardRecordCursor {
  id: number;
  timestamp: string;
}

export interface UsageRecord {
  id: number;
  timestamp: string;
  request_id?: string | null;
  team_id: string;
  router: string;
  matched_rule?: string | null;
  final_channel: string;
  channel: string;
  model: string;
  input_tokens: number;
  output_tokens: number;
  latency_ms?: number | null;
  fallback_triggered: boolean;
  status: string;
  status_code?: number | null;
  error_message?: string | null;
  provider_trace_id?: string | null;
  provider_error_body?: string | null;
}

export interface DashboardAnalyticsResponse {
  generated_at: string;
  range: DashboardRange;
  filter_options: DashboardFilterOptions;
  overview: DashboardOverview;
  trend: DashboardTrendSection;
  topology: {
    nodes: DashboardTopologyNode[];
    links: DashboardTopologyLink[];
    flows: DashboardFlowSummary[];
    render_mode: string;
  };
  team_usage: {
    leaderboard: DashboardTeamLeaderboardItem[];
    model_usage: DashboardTeamModelUsageItem[];
  };
  system_reliability: {
    error_rate_trend: DashboardTrendPoint[];
    channel_latency: DashboardChannelLatencyItem[];
  };
  model_router: {
    model_share: DashboardShareItem[];
    router_summary: DashboardShareItem[];
    channel_summary: DashboardShareItem[];
  };
  records_meta: {
    total: number;
    latest_cursor: DashboardRecordCursor | null;
  };
}

export interface DashboardRecordsResponse {
  data: UsageRecord[];
  total: number;
  limit: number;
  offset: number;
  latest_cursor: DashboardRecordCursor | null;
  new_records: number;
}
