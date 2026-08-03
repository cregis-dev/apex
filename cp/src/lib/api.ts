import { authHeaders } from './auth.ts'
import type {
  AnalyticsResponse, AnalyticsParams,
  RecordsResponse, RecordsParams,
  AdminListResponse, AdminChannel, AdminRouter, AdminTeam,
  ChannelApiKeyEntry, TeamApiKeyEntry, ProviderTemplate,
  CreateTeamRequest, UpdateTeamRequest,
  CreateChannelRequest, UpdateChannelRequest,
  CreateRouterRequest, UpdateRouterRequest,
  CpInfo, PricingConfig, LogEntry,
} from './types.ts'

class ApiError extends Error {
  constructor(public status: number, message: string) {
    super(message)
    this.name = 'ApiError'
  }
}

async function req<T>(method: string, path: string, body?: unknown): Promise<T> {
  const res = await fetch(path, {
    method,
    headers: {
      ...authHeaders(),
      ...(body ? { 'Content-Type': 'application/json' } : {}),
    },
    body: body ? JSON.stringify(body) : undefined,
  })

  if (!res.ok) {
    const text = await res.text().catch(() => res.statusText)
    throw new ApiError(res.status, text)
  }

  return res.json() as Promise<T>
}

function qs(params: Record<string, string | number | undefined | null>): string {
  const p = new URLSearchParams()
  for (const [k, v] of Object.entries(params)) {
    if (v !== undefined && v !== '') p.set(k, String(v))
  }
  const s = p.toString()
  return s ? `?${s}` : ''
}

export const api = {
  analytics: (params: AnalyticsParams = {}) =>
    req<AnalyticsResponse>('GET', `/api/dashboard/analytics${qs(params as Record<string, string | number | undefined>)}`),

  records: (params: RecordsParams = {}) =>
    req<RecordsResponse>('GET', `/api/dashboard/records${qs(params as Record<string, string | number | undefined>)}`),

  channels: () =>
    req<AdminListResponse<AdminChannel>>('GET', '/admin/channels'),

  channelApiKeys: () =>
    req<AdminListResponse<ChannelApiKeyEntry>>('GET', '/admin/channels/api_keys'),

  providerTemplates: () =>
    req<AdminListResponse<ProviderTemplate>>('GET', '/api/cp/provider-templates'),

  createChannel: (body: CreateChannelRequest) =>
    req<AdminChannel>('POST', '/admin/channels', body),

  updateChannel: (name: string, body: UpdateChannelRequest) =>
    req<AdminChannel>('PATCH', `/admin/channels/${encodeURIComponent(name)}`, body),

  deleteChannel: (name: string) =>
    req<{ deleted: string }>('DELETE', `/admin/channels/${encodeURIComponent(name)}`),

  routers: () =>
    req<AdminListResponse<AdminRouter>>('GET', '/admin/routers'),

  createRouter: (body: CreateRouterRequest) =>
    req<AdminRouter>('POST', '/admin/routers', body),

  updateRouter: (name: string, body: UpdateRouterRequest) =>
    req<AdminRouter>('PATCH', `/admin/routers/${encodeURIComponent(name)}`, body),

  deleteRouter: (name: string) =>
    req<{ deleted: string }>('DELETE', `/admin/routers/${encodeURIComponent(name)}`),

  teams: () =>
    req<AdminListResponse<AdminTeam>>('GET', '/admin/teams'),

  teamApiKeys: () =>
    req<AdminListResponse<TeamApiKeyEntry>>('GET', '/admin/teams/api_keys'),

  // Reveal the *unmasked* api_key for a single team (admin-only).
  revealTeamApiKey: (id: string) =>
    req<{ id: string; api_key: string }>('GET', `/admin/teams/${encodeURIComponent(id)}/api_key`),

  createTeam: (body: CreateTeamRequest) =>
    req<AdminTeam>('POST', '/admin/teams', body),

  updateTeam: (id: string, body: UpdateTeamRequest) =>
    req<AdminTeam>('PATCH', `/admin/teams/${encodeURIComponent(id)}`, body),

  deleteTeam: (id: string) =>
    req<{ deleted: string }>('DELETE', `/admin/teams/${encodeURIComponent(id)}`),

  cpInfo: () =>
    req<CpInfo>('GET', '/api/cp/info'),

  pricing: () =>
    req<PricingConfig>('GET', '/admin/pricing'),

  savePricing: (body: PricingConfig) =>
    req<PricingConfig>('PUT', '/admin/pricing', body),
}

/**
 * Subscribe to the gateway's live log stream.
 *
 * Consumes SSE over `fetch` rather than `EventSource`: the control plane
 * authenticates with an `Authorization` header, and `EventSource` cannot set
 * headers. Replays a backlog first, then streams live lines.
 *
 * Returns an unsubscribe function that aborts the in-flight request.
 */
export function streamLogs(
  params: { level?: string; after_seq?: number; limit?: number },
  handlers: {
    onEntry: (entry: LogEntry) => void
    onOpen?: () => void
    onError?: (err: unknown) => void
  },
): () => void {
  const controller = new AbortController()

  void (async () => {
    try {
      const res = await fetch(`/api/cp/logs/stream${qs(params)}`, {
        headers: { ...authHeaders(), Accept: 'text/event-stream' },
        signal: controller.signal,
      })
      if (!res.ok || !res.body) {
        throw new ApiError(res.status, await res.text().catch(() => res.statusText))
      }
      handlers.onOpen?.()

      const reader = res.body.getReader()
      const decoder = new TextDecoder()
      let buffer = ''

      for (;;) {
        const { done, value } = await reader.read()
        if (done) break
        buffer += decoder.decode(value, { stream: true })

        // SSE frames are separated by a blank line; keep the trailing partial.
        const frames = buffer.split('\n\n')
        buffer = frames.pop() ?? ''
        for (const frame of frames) {
          const data = frame
            .split('\n')
            .filter((l) => l.startsWith('data:'))
            .map((l) => l.slice(5).trimStart())
            .join('\n')
          if (!data) continue // keep-alive comment frames carry no data
          try {
            handlers.onEntry(JSON.parse(data) as LogEntry)
          } catch {
            // Ignore a malformed frame rather than tearing down the stream.
          }
        }
      }
    } catch (err) {
      if (!controller.signal.aborted) handlers.onError?.(err)
    }
  })()

  return () => controller.abort()
}

export { ApiError }
