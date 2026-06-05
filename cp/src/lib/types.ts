// ---------------------------------------------------------------------------
// Matches Rust structs in src/server.rs and src/config.rs exactly.
// ---------------------------------------------------------------------------

// --- /api/dashboard/analytics ---

export interface OverviewDelta {
  total_requests: number
  total_tokens: number
  avg_latency_ms: number
  success_rate: number
}

export interface Overview {
  total_requests: number
  total_tokens: number
  input_tokens: number
  output_tokens: number
  avg_latency_ms: number
  success_rate: number
  delta: OverviewDelta
}

export interface TrendPoint {
  bucket: string
  label: string
  requests: number
  input_tokens: number
  output_tokens: number
  total_tokens: number
  error_rate: number
  avg_latency_ms: number
  success_rate: number
}

export interface TrendSection {
  unit: string
  points: TrendPoint[]
}

export interface ShareItem {
  name: string
  requests: number
  total_tokens: number
  percentage: number
}

export interface ModelRouterSection {
  model_share: ShareItem[]
  router_summary: ShareItem[]
  channel_summary: ShareItem[]
}

export interface TopologyNode {
  name: string
  kind: string
}

export interface TopologyLink {
  source: number
  target: number
  value: number
  total_tokens: number
}

export interface FlowSummary {
  team_id: string
  router: string
  channel: string
  model: string
  requests: number
  total_tokens: number
}

export interface TopologySection {
  nodes: TopologyNode[]
  links: TopologyLink[]
  flows: FlowSummary[]
  render_mode: string
}

export interface TeamLeaderboardItem {
  team_id: string
  total_requests: number
  total_tokens: number
}

export interface TeamModelUsageItem {
  team_id: string
  model: string
  total_requests: number
  total_tokens: number
}

export interface TeamUsageSection {
  leaderboard: TeamLeaderboardItem[]
  model_usage: TeamModelUsageItem[]
}

export interface ChannelLatencyItem {
  channel: string
  total_requests: number
  avg_latency_ms: number
  p95_latency_ms: number
}

export interface SystemReliabilitySection {
  error_rate_trend: TrendPoint[]
  channel_latency: ChannelLatencyItem[]
}

export interface FilterOptions {
  teams: string[]
  models: string[]
  routers: string[]
  channels: string[]
}

export interface RecordCursor {
  id: number
  timestamp: string
}

export interface AnalyticsResponse {
  generated_at: string
  range: string
  filter_options: FilterOptions
  overview: Overview
  trend: TrendSection
  topology: TopologySection
  team_usage: TeamUsageSection
  system_reliability: SystemReliabilitySection
  model_router: ModelRouterSection
  records_meta: { total: number; latest_cursor: RecordCursor | null }
}

// --- /api/dashboard/records ---

export interface UsageRecord {
  id: number
  timestamp: string
  request_id: string | null
  team_id: string
  router: string
  matched_rule: string | null
  final_channel: string
  channel: string
  model: string
  input_tokens: number
  output_tokens: number
  latency_ms: number | null
  fallback_triggered: boolean
  status: string
  status_code: number | null
  error_message: string | null
  provider_trace_id: string | null
  provider_error_body: string | null
}

export interface RecordsResponse {
  data: UsageRecord[]
  total: number
  limit: number
  offset: number
  latest_cursor: RecordCursor | null
  new_records: number
}

// --- /admin/channels ---

export type ProviderType =
  | 'openai' | 'anthropic' | 'gemini' | 'custom_dual'
  | 'deepseek' | 'moonshot' | 'minimax' | 'ollama'
  | 'jina' | 'openrouter' | 'zai'

export interface AdminChannel {
  name: string
  provider_type: ProviderType
  base_url: string
  anthropic_base_url: string | null
}

export interface CreateChannelRequest {
  name: string
  provider_type: ProviderType
  base_url: string
  api_key: string
  anthropic_base_url?: string | null
  headers?: Record<string, string> | null
  model_map?: Record<string, string> | null
}

export interface UpdateChannelRequest {
  provider_type?: ProviderType
  base_url?: string
  /** Omit to keep current. Empty string is rejected. */
  api_key?: string
  anthropic_base_url?: string | null
  headers?: Record<string, string> | null
  model_map?: Record<string, string> | null
}

/** GET /admin/channels/api_keys entry. */
export interface ChannelApiKeyEntry {
  name: string
  /** Server-side masked form, e.g. "sk-…ab0c". */
  api_key: string
}

/** GET /api/cp/provider-templates entry — default URLs per provider type. */
export interface ProviderTemplate {
  provider_type: ProviderType
  base_url: string
  anthropic_base_url: string | null
}

export interface AdminListResponse<T> {
  object: 'list'
  data: T[]
}

// --- /admin/routers ---

export interface TargetChannel {
  name: string
  weight: number
}

export interface MatchSpec {
  models: string[]
}

export interface RouterRule {
  match: MatchSpec
  channels: TargetChannel[]
  strategy: string
}

export interface AdminRouter {
  name: string
  rules: RouterRule[]
  channels?: TargetChannel[]
  strategy?: string
  fallback_channels?: string[]
}

export interface RouterRuleInput {
  models: string[]
  channels: { name: string; weight?: number }[]
  strategy?: string
}

export interface CreateRouterRequest {
  name: string
  rules: RouterRuleInput[]
  fallback_channels?: string[]
}

export interface UpdateRouterRequest {
  rules?: RouterRuleInput[]
  fallback_channels?: string[]
}

// --- /admin/teams ---

export interface RateLimit {
  rpm: number | null
  tpm: number | null
}

export interface TeamPolicy {
  allowed_routers: string[]
  allowed_models: string[] | null
  rate_limit: RateLimit | null
}

export interface AdminTeam {
  id: string
  /** Optional group label. Empty string and null both render as "Default". */
  group: string | null
  /** Defaults to true. When false, all model requests from this team are rejected. */
  enabled: boolean
  policy: TeamPolicy
  /**
   * Only present on the create response — the *unmasked* full api_key,
   * shown to the operator once. List/update/delete responses never include
   * the key; fetch the masked form from GET /admin/teams/api_keys.
   */
  api_key?: string
  api_key_revealed?: boolean
}

/** GET /admin/teams/api_keys entry. */
export interface TeamApiKeyEntry {
  id: string
  /** Server-side masked form, e.g. "sk-…ab0c". */
  api_key: string
}

export interface CreateTeamRequest {
  id: string
  group?: string | null
  enabled?: boolean
  api_key?: string
  allowed_routers?: string[]
  allowed_models?: string[] | null
  rate_limit?: { rpm: number | null; tpm: number | null } | null
}

export interface UpdateTeamRequest {
  group?: string | null
  enabled?: boolean
  allowed_routers?: string[]
  allowed_models?: string[] | null
  rate_limit?: { rpm: number | null; tpm: number | null } | null
}

// --- /api/cp/info ---

export interface CpInfo {
  version: string
  listen: string
  auth_required: boolean
  auth_key_count: number
  cors_origins: string[]
  timeouts: { connect_ms: number; request_ms: number; response_ms: number }
  retries: { max_attempts: number; backoff_ms: number }
  channels: number
  routers: number
  teams: number
  metrics_enabled: boolean
  hot_reload: boolean
}

// --- query params ---

export type TimeRange = '1h' | '24h' | '7d' | '30d'

export interface AnalyticsParams {
  range?: TimeRange
  team_id?: string
  router?: string
  channel?: string
  model?: string
}

export interface RecordsParams extends AnalyticsParams {
  limit?: number
  offset?: number
  since_timestamp?: string
  since_id?: number
  status?: string
}
