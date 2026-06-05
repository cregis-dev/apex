import { authHeaders } from './auth.ts'
import type {
  AnalyticsResponse, AnalyticsParams,
  RecordsResponse, RecordsParams,
  AdminListResponse, AdminChannel, AdminRouter, AdminTeam,
  ChannelApiKeyEntry, TeamApiKeyEntry, ProviderTemplate,
  CreateTeamRequest, UpdateTeamRequest,
  CreateChannelRequest, UpdateChannelRequest,
  CreateRouterRequest, UpdateRouterRequest,
  CpInfo,
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

  createTeam: (body: CreateTeamRequest) =>
    req<AdminTeam>('POST', '/admin/teams', body),

  updateTeam: (id: string, body: UpdateTeamRequest) =>
    req<AdminTeam>('PATCH', `/admin/teams/${encodeURIComponent(id)}`, body),

  deleteTeam: (id: string) =>
    req<{ deleted: string }>('DELETE', `/admin/teams/${encodeURIComponent(id)}`),

  cpInfo: () =>
    req<CpInfo>('GET', '/api/cp/info'),
}

export { ApiError }
