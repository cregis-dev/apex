import { useState } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import Topbar from '../components/Topbar.tsx'
import Empty from '../components/Empty.tsx'
import Icon from '../components/Icon.tsx'
import RateLimitEditor from '../components/RateLimitEditor.tsx'
import { useToast } from '../components/Toast.tsx'
import { api } from '../lib/api.ts'
import type { AdminTeam, RateLimit } from '../lib/types.ts'

const WARN_INK = 'oklch(0.42 0.1 70)'

function fmt(n: number | null): string {
  if (n == null) return '—'
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`
  if (n >= 1_000) return `${(n / 1_000).toFixed(1)}k`
  return String(n)
}

function QuotaBar({ pct }: { pct: number }) {
  const color = pct > 85 ? 'var(--err)' : pct > 70 ? 'var(--warn)' : 'var(--brand)'
  return (
    <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
      <div style={{ flex: 1, height: 5, background: 'var(--surface-2)', borderRadius: 3, overflow: 'hidden', minWidth: 80 }}>
        <div style={{ width: `${Math.min(100, pct)}%`, height: '100%', background: color, borderRadius: 3, transition: 'width 300ms' }} />
      </div>
      <span style={{ fontSize: 11, fontFamily: 'var(--font-mono)', color, width: 32, textAlign: 'right' }}>{pct.toFixed(0)}%</span>
    </div>
  )
}

function Stat({ label, value, sub, accent }: { label: string; value: string; sub?: string; accent?: string }) {
  return (
    <div className="card" style={{ padding: '16px 18px' }}>
      <div style={{ fontSize: 12, color: 'var(--muted)', marginBottom: 8 }}>{label}</div>
      <div style={{ fontSize: 24, fontWeight: 600, letterSpacing: '-0.02em', color: accent ?? 'var(--ink)' }}>{value}</div>
      {sub && <div style={{ fontSize: 12, color: 'var(--muted)', marginTop: 4 }}>{sub}</div>}
    </div>
  )
}

export default function RateLimitsPage() {
  const qc = useQueryClient()
  const { push } = useToast()
  const [query, setQuery] = useState('')
  const [editorTeam, setEditorTeam] = useState<AdminTeam | null>(null)
  const [editorError, setEditorError] = useState<string | undefined>()

  const { data: teamsData, isLoading, error, refetch } = useQuery({
    queryKey: ['teams'],
    queryFn: api.teams,
  })

  const { data: analyticsData } = useQuery({
    queryKey: ['analytics', '1h'],
    queryFn: () => api.analytics({ range: '1h' }),
  })

  const saveMutation = useMutation({
    mutationFn: ({ id, rate_limit }: { id: string; rate_limit: RateLimit | null }) =>
      api.updateTeam(id, { rate_limit }),
    onSuccess: (updated) => {
      void qc.invalidateQueries({ queryKey: ['teams'] })
      const has = updated.policy.rate_limit?.rpm != null || updated.policy.rate_limit?.tpm != null
      push(`Rate limit ${has ? 'saved' : 'removed'} for "${updated.id}"`, 'ok')
      setEditorTeam(null)
    },
    onError: (err: unknown) => {
      setEditorError(err instanceof Error ? err.message : 'Failed to update rate limit')
    },
  })

  const teams = teamsData?.data ?? []
  const teamsWithLimits = teams.filter((t) => t.policy.rate_limit?.rpm != null || t.policy.rate_limit?.tpm != null)
  const teamsNoLimit = teams.filter((t) => !t.policy.rate_limit?.rpm && !t.policy.rate_limit?.tpm)

  // Derive current usage from analytics leaderboard
  const usageMap = new Map(
    analyticsData?.team_usage.leaderboard.map((t) => [t.team_id, t.total_requests]) ?? []
  )
  const usageOf = (id: string) => usageMap.get(id) ?? 0

  // Summary figures
  const totalReq1h = teams.reduce((s, t) => s + usageOf(t.id), 0)
  const activeNoLimit = teamsNoLimit.filter((t) => usageOf(t.id) > 0).length
  const coverage = teams.length ? Math.round((teamsWithLimits.length / teams.length) * 100) : 0

  // Search + sort (active teams float to the top of the unthrottled list)
  const q = query.trim().toLowerCase()
  const matches = (id: string) => id.toLowerCase().includes(q)
  const withLimitsView = teamsWithLimits.filter((t) => matches(t.id))
  const noLimitView = teamsNoLimit
    .filter((t) => matches(t.id))
    .sort((a, b) => usageOf(b.id) - usageOf(a.id) || a.id.localeCompare(b.id))
  const nothingMatches = q !== '' && withLimitsView.length === 0 && noLimitView.length === 0

  function openEditor(team: AdminTeam) {
    setEditorError(undefined)
    setEditorTeam(team)
  }

  function submitEditor(rate_limit: RateLimit | null) {
    if (!editorTeam) return
    setEditorError(undefined)
    saveMutation.mutate({ id: editorTeam.id, rate_limit })
  }

  return (
    <>
      <Topbar
        breadcrumbs={[{ label: 'Access' }, { label: 'Rate Limits' }]}
        actions={
          <button className="btn btn-sm" onClick={() => void refetch()} title="Refresh">
            <Icon name="refresh" size={13} />
            Refresh
          </button>
        }
      />
      <div className="page-pad">
        <div className="page-head">
          <h1 className="page-title">Rate Limits &amp; Quotas</h1>
          <p className="page-sub">Global, per-team, and per-key throttles. Token-bucket enforced at the gateway edge.</p>
        </div>

        <div style={{
          padding: '10px 14px', marginBottom: 20,
          background: 'var(--info-soft)', borderRadius: 'var(--r-md)',
          fontSize: 13, color: 'var(--info)',
          display: 'flex', alignItems: 'center', gap: 8,
        }}>
          <Icon name="info" size={15} style={{ flexShrink: 0 }} />
          Limits are per-team. Click a team to set, change, or remove its limit — usage shown is for the last 1 hour.
        </div>

        {isLoading && (
          <div style={{ display: 'flex', justifyContent: 'center', padding: 48 }}>
            <span className="spinner" style={{ width: 20, height: 20 }} />
          </div>
        )}

        {error && (
          <div style={{ padding: '12px 16px', background: 'var(--err-soft)', color: 'var(--err)', borderRadius: 'var(--r-md)', fontSize: 13 }}>
            Failed to load rate limits.
          </div>
        )}

        {!isLoading && !error && teams.length === 0 && (
          <Empty icon="gauge" title="No teams configured" sub="Rate limits are per-team. Add teams to your config.json first." />
        )}

        {teams.length > 0 && (
          <>
            {/* Summary */}
            <div style={{ display: 'grid', gridTemplateColumns: 'repeat(4, 1fr)', gap: 16, marginBottom: 24 }}>
              <Stat label="Total teams" value={String(teams.length)} />
              <Stat
                label="With rate limits"
                value={String(teamsWithLimits.length)}
                sub={`${coverage}% coverage`}
              />
              <Stat
                label="Without rate limits"
                value={String(teamsNoLimit.length)}
                sub={activeNoLimit > 0 ? `${activeNoLimit} with live traffic` : 'all idle'}
                accent={activeNoLimit > 0 ? WARN_INK : undefined}
              />
              <Stat label="Requests" value={totalReq1h.toLocaleString()} sub="last 1 hour" />
            </div>

            {/* Search */}
            <div style={{ display: 'flex', justifyContent: 'flex-end', marginBottom: 16 }}>
              <div style={{ position: 'relative', width: 260, maxWidth: '100%' }}>
                <Icon
                  name="search"
                  size={14}
                  style={{ position: 'absolute', left: 10, top: '50%', transform: 'translateY(-50%)', color: 'var(--muted-2)' }}
                />
                <input
                  className="input"
                  placeholder="Filter teams…"
                  value={query}
                  onChange={(e) => setQuery(e.target.value)}
                  style={{ width: '100%', paddingLeft: 30 }}
                />
              </div>
            </div>

            {nothingMatches && (
              <Empty icon="search" title="No teams match" sub={`Nothing matches “${query.trim()}”.`} />
            )}

            {/* Teams with limits */}
            {withLimitsView.length > 0 && (
              <div style={{ marginBottom: 24 }}>
                <div className="section-title">Teams with rate limits</div>
                <div className="card">
                  <table className="table">
                    <thead>
                      <tr>
                        <th>Team</th>
                        <th style={{ textAlign: 'right' }}>RPM limit</th>
                        <th style={{ textAlign: 'right' }}>TPM limit</th>
                        <th style={{ textAlign: 'right' }}>Req (1h)</th>
                        <th style={{ width: 160 }}>Usage</th>
                        <th style={{ width: 56 }} />
                      </tr>
                    </thead>
                    <tbody>
                      {withLimitsView.map((team) => {
                        const rl = team.policy.rate_limit!
                        const used = usageOf(team.id)
                        // Approximate: requests in 1h vs rpm*60
                        const rpmCap = rl.rpm != null ? rl.rpm * 60 : null
                        const pct = rpmCap ? Math.min(100, (used / rpmCap) * 100) : 0
                        return (
                          <tr key={team.id} className="row-hover">
                            <td style={{ fontWeight: 500 }}>{team.id}</td>
                            <td style={{ textAlign: 'right', fontFamily: 'var(--font-mono)', fontSize: 12 }}>
                              {fmt(rl.rpm)}
                            </td>
                            <td style={{ textAlign: 'right', fontFamily: 'var(--font-mono)', fontSize: 12 }}>
                              {fmt(rl.tpm)}
                            </td>
                            <td style={{ textAlign: 'right', fontFamily: 'var(--font-mono)', fontSize: 12 }}>
                              {used.toLocaleString()}
                            </td>
                            <td>
                              {rpmCap ? <QuotaBar pct={pct} /> : <span className="muted" style={{ fontSize: 12 }}>—</span>}
                            </td>
                            <td style={{ textAlign: 'right' }}>
                              <button
                                className="btn btn-ghost btn-sm"
                                title="Edit rate limit"
                                onClick={() => openEditor(team)}
                                style={{ padding: '0 8px' }}
                              >
                                <Icon name="edit" size={13} />
                              </button>
                            </td>
                          </tr>
                        )
                      })}
                    </tbody>
                  </table>
                </div>
              </div>
            )}

            {/* Teams without limits */}
            {noLimitView.length > 0 && (
              <div>
                <div className="section-title" style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between' }}>
                  <span>Teams without rate limits · {noLimitView.length}</span>
                  <span style={{
                    display: 'flex', alignItems: 'center', gap: 6,
                    textTransform: 'none', letterSpacing: 'normal', fontSize: 11,
                  }}>
                    <span className="dot" style={{ background: 'var(--warn)' }} />
                    has traffic · req in last 1h
                  </span>
                </div>
                <div className="card" style={{ padding: 6 }}>
                  <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fill, minmax(210px, 1fr))', gap: 2 }}>
                    {noLimitView.map((t) => {
                      const used = usageOf(t.id)
                      const active = used > 0
                      return (
                        <button
                          key={t.id}
                          type="button"
                          className="rl-team"
                          title="Set rate limit"
                          onClick={() => openEditor(t)}
                          style={{
                            display: 'flex', alignItems: 'center', gap: 9, width: '100%',
                            padding: '8px 10px', borderRadius: 'var(--r-sm)', textAlign: 'left',
                          }}
                        >
                          <span className="dot" style={{ background: active ? 'var(--warn)' : 'var(--muted-2)' }} />
                          <span style={{
                            flex: 1, fontSize: 13, fontWeight: 500,
                            color: active ? 'var(--ink)' : 'var(--ink-2)',
                            overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap',
                          }}>
                            {t.id}
                          </span>
                          <span style={{
                            fontSize: 12, fontFamily: 'var(--font-mono)',
                            color: active ? WARN_INK : 'var(--muted-2)',
                          }}>
                            {used.toLocaleString()}
                          </span>
                          <Icon name="plus" size={13} className="rl-edit" style={{ color: 'var(--muted)', flexShrink: 0 }} />
                        </button>
                      )
                    })}
                  </div>
                </div>
              </div>
            )}
          </>
        )}
      </div>

      {editorTeam && (
        <RateLimitEditor
          key={editorTeam.id}
          team={editorTeam}
          busy={saveMutation.isPending}
          error={editorError}
          onCancel={() => setEditorTeam(null)}
          onSubmit={submitEditor}
        />
      )}
    </>
  )
}
