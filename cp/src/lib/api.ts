import { authHeaders } from './auth.ts'
import type {
  AnalyticsResponse, AnalyticsParams,
  RecordsResponse, RecordsParams,
  AdminListResponse, AdminChannel, AdminRouter, AdminTeam,
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

  routers: () =>
    req<AdminListResponse<AdminRouter>>('GET', '/admin/routers'),

  teams: () =>
    req<AdminListResponse<AdminTeam>>('GET', '/admin/teams'),

  cpInfo: () =>
    req<CpInfo>('GET', '/api/cp/info'),
}

export { ApiError }
