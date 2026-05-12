import { useQuery } from '@tanstack/react-query'
import Topbar from '../components/Topbar.tsx'
import Empty from '../components/Empty.tsx'
import { api } from '../lib/api.ts'

function fmt(n: number | null): string {
  if (n == null) return '—'
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`
  if (n >= 1_000) return `${(n / 1_000).toFixed(0)}k`
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

export default function RateLimitsPage() {
  const { data: teamsData, isLoading, error } = useQuery({
    queryKey: ['teams'],
    queryFn: api.teams,
  })

  const { data: analyticsData } = useQuery({
    queryKey: ['analytics', '1h'],
    queryFn: () => api.analytics({ range: '1h' }),
  })

  const teams = teamsData?.data ?? []
  const teamsWithLimits = teams.filter((t) => t.policy.rate_limit?.rpm != null || t.policy.rate_limit?.tpm != null)
  const teamsNoLimit = teams.filter((t) => !t.policy.rate_limit?.rpm && !t.policy.rate_limit?.tpm)

  // Derive current usage from analytics leaderboard
  const usageMap = new Map(
    analyticsData?.team_usage.leaderboard.map((t) => [t.team_id, t.total_requests]) ?? []
  )

  return (
    <>
      <Topbar breadcrumbs={[{ label: 'Access' }, { label: 'Rate Limits' }]} />
      <div className="page-pad">
        <div className="page-head">
          <h1 className="page-title">Rate Limits &amp; Quotas</h1>
          <p className="page-sub">Global, per-team, and per-key throttles. Token-bucket enforced at the gateway edge.</p>
        </div>

        <div style={{
          padding: '10px 14px', marginBottom: 20,
          background: 'var(--info-soft)', borderRadius: 'var(--r-md)',
          fontSize: 13, color: 'var(--info)',
        }}>
          Rate limits are configured per-team in the gateway config file. Usage shown is for the last 1 hour.
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
            {/* Teams with limits */}
            {teamsWithLimits.length > 0 && (
              <div style={{ marginBottom: 20 }}>
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
                      </tr>
                    </thead>
                    <tbody>
                      {teamsWithLimits.map((team) => {
                        const rl = team.policy.rate_limit!
                        const used = usageMap.get(team.id) ?? 0
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
                          </tr>
                        )
                      })}
                    </tbody>
                  </table>
                </div>
              </div>
            )}

            {/* Teams without limits */}
            {teamsNoLimit.length > 0 && (
              <div>
                <div className="section-title">Teams without rate limits</div>
                <div className="card" style={{ padding: '12px 20px' }}>
                  <div style={{ display: 'flex', gap: 8, flexWrap: 'wrap' }}>
                    {teamsNoLimit.map((t) => (
                      <div key={t.id} style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
                        <span className="badge">{t.id}</span>
                        <span style={{ fontSize: 12, color: 'var(--muted)' }}>
                          {(usageMap.get(t.id) ?? 0).toLocaleString()} req/1h
                        </span>
                      </div>
                    ))}
                  </div>
                </div>
              </div>
            )}
          </>
        )}
      </div>
    </>
  )
}
