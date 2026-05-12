import { useQuery } from '@tanstack/react-query'
import Topbar from '../components/Topbar.tsx'
import Empty from '../components/Empty.tsx'
import { api } from '../lib/api.ts'

function fmt(n: number | null | undefined): string {
  if (n == null) return '—'
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`
  if (n >= 1_000) return `${(n / 1_000).toFixed(0)}k`
  return String(n)
}

function Monogram({ id }: { id: string }) {
  const letters = id.replace(/[^a-zA-Z0-9]/g, '').slice(0, 2).toUpperCase()
  return (
    <div style={{
      width: 28, height: 28, borderRadius: 6,
      background: 'oklch(0.75 0.06 55)',
      display: 'flex', alignItems: 'center', justifyContent: 'center',
      color: '#fff', fontSize: 11, fontWeight: 600, flexShrink: 0,
    }}>
      {letters}
    </div>
  )
}

export default function TeamsPage() {
  const { data, isLoading, error } = useQuery({
    queryKey: ['teams'],
    queryFn: api.teams,
  })

  const teams = data?.data ?? []

  return (
    <>
      <Topbar breadcrumbs={[{ label: 'Access' }, { label: 'Teams' }]} />
      <div className="page-pad">
        <div className="page-head">
          <h1 className="page-title">Teams</h1>
          <p className="page-sub">Multi-tenant boundaries. Each team has its own keys, quotas, and model allowlist.</p>
        </div>

        {isLoading && (
          <div style={{ display: 'flex', justifyContent: 'center', padding: 48 }}>
            <span className="spinner" style={{ width: 20, height: 20 }} />
          </div>
        )}

        {error && (
          <div style={{ padding: '12px 16px', background: 'var(--err-soft)', color: 'var(--err)', borderRadius: 'var(--r-md)', fontSize: 13 }}>
            Failed to load teams.
          </div>
        )}

        {!isLoading && !error && (
          <div className="card">
            {teams.length === 0 ? (
              <Empty icon="users" title="No teams configured" sub="Add teams to your config.json to enable multi-tenant access control." />
            ) : (
              <table className="table">
                <thead>
                  <tr>
                    <th>Team</th>
                    <th>API Key</th>
                    <th>RPM limit</th>
                    <th>TPM limit</th>
                    <th>Allowed routers</th>
                    <th>Allowed models</th>
                  </tr>
                </thead>
                <tbody>
                  {teams.map((team) => {
                    const rl = team.policy.rate_limit
                    return (
                      <tr key={team.id} className="row-hover">
                        <td>
                          <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
                            <Monogram id={team.id} />
                            <div>
                              <div style={{ fontWeight: 500 }}>{team.id}</div>
                              <div style={{ fontSize: 11, fontFamily: 'var(--font-mono)', color: 'var(--muted)' }}>{team.id}</div>
                            </div>
                          </div>
                        </td>
                        <td style={{ fontFamily: 'var(--font-mono)', fontSize: 12 }}>{team.api_key || '—'}</td>
                        <td style={{ fontFamily: 'var(--font-mono)', fontSize: 12 }}>
                          {rl?.rpm != null ? fmt(rl.rpm) : <span className="muted">—</span>}
                        </td>
                        <td style={{ fontFamily: 'var(--font-mono)', fontSize: 12 }}>
                          {rl?.tpm != null ? fmt(rl.tpm) : <span className="muted">—</span>}
                        </td>
                        <td>
                          {team.policy.allowed_routers.length > 0 ? (
                            <div style={{ display: 'flex', gap: 4, flexWrap: 'wrap' }}>
                              {team.policy.allowed_routers.map((r) => (
                                <span key={r} className="badge">{r}</span>
                              ))}
                            </div>
                          ) : <span className="muted" style={{ fontSize: 12 }}>all</span>}
                        </td>
                        <td>
                          {(team.policy.allowed_models ?? []).length > 0 ? (
                            <div style={{ display: 'flex', gap: 4, flexWrap: 'wrap' }}>
                              {(team.policy.allowed_models ?? []).slice(0, 3).map((m) => (
                                <span key={m} className="badge mono" style={{ fontSize: 11 }}>{m}</span>
                              ))}
                              {(team.policy.allowed_models ?? []).length > 3 && (
                                <span className="badge">+{(team.policy.allowed_models ?? []).length - 3}</span>
                              )}
                            </div>
                          ) : <span className="muted" style={{ fontSize: 12 }}>all</span>}
                        </td>
                      </tr>
                    )
                  })}
                </tbody>
              </table>
            )}
          </div>
        )}
      </div>
    </>
  )
}
