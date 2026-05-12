import { useState } from 'react'
import { useQuery } from '@tanstack/react-query'
import Topbar from '../components/Topbar.tsx'
import Icon from '../components/Icon.tsx'
import Empty from '../components/Empty.tsx'
import { api } from '../lib/api.ts'
import { useToast } from '../components/Toast.tsx'

export default function KeysPage() {
  const { data, isLoading, error } = useQuery({
    queryKey: ['teams'],
    queryFn: api.teams,
  })
  const { push } = useToast()
  const [revealed, setRevealed] = useState<Set<string>>(new Set())

  const teams = data?.data ?? []

  function toggleReveal(id: string) {
    setRevealed((s) => {
      const n = new Set(s)
      n.has(id) ? n.delete(id) : n.add(id)
      return n
    })
  }

  function copyKey(id: string, key: string) {
    void navigator.clipboard.writeText(key).then(() => push('API key copied', 'ok'))
    void id
  }

  return (
    <>
      <Topbar breadcrumbs={[{ label: 'Access' }, { label: 'API Keys' }]} />
      <div className="page-pad">
        <div className="page-head">
          <h1 className="page-title">API Keys</h1>
          <p className="page-sub">Issue, scope and revoke keys. Keys inherit their team's quotas and model allowlist.</p>
        </div>

        <div style={{
          padding: '10px 14px', marginBottom: 16,
          background: 'var(--info-soft)', borderRadius: 'var(--r-md)',
          fontSize: 13, color: 'var(--info)',
          display: 'flex', alignItems: 'center', gap: 8,
        }}>
          <Icon name="info" size={14} />
          API keys are managed in the gateway config file. Keys shown here are masked for security.
        </div>

        {isLoading && (
          <div style={{ display: 'flex', justifyContent: 'center', padding: 48 }}>
            <span className="spinner" style={{ width: 20, height: 20 }} />
          </div>
        )}

        {error && (
          <div style={{ padding: '12px 16px', background: 'var(--err-soft)', color: 'var(--err)', borderRadius: 'var(--r-md)', fontSize: 13 }}>
            Failed to load keys.
          </div>
        )}

        {!isLoading && !error && (
          <div className="card">
            {teams.length === 0 ? (
              <Empty icon="key" title="No teams configured" sub="API keys are bound to teams. Add teams to your config.json first." />
            ) : (
              <table className="table">
                <thead>
                  <tr>
                    <th>Team</th>
                    <th>API Key</th>
                    <th>Rate limit</th>
                    <th>Allowed models</th>
                    <th style={{ width: 80 }}></th>
                  </tr>
                </thead>
                <tbody>
                  {teams.map((team) => {
                    const show = revealed.has(team.id)
                    const maskedKey = team.api_key
                      ? show ? team.api_key : `${team.api_key.slice(0, 8)}${'•'.repeat(Math.max(0, team.api_key.length - 8))}`
                      : '—'
                    const rl = team.policy.rate_limit
                    return (
                      <tr key={team.id} className="row-hover">
                        <td>
                          <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
                            <div style={{
                              width: 24, height: 24, borderRadius: 4,
                              background: 'oklch(0.75 0.06 55)',
                              display: 'flex', alignItems: 'center', justifyContent: 'center',
                              color: '#fff', fontSize: 10, fontWeight: 600, flexShrink: 0,
                            }}>
                              {team.id.slice(0, 2).toUpperCase()}
                            </div>
                            <span style={{ fontWeight: 500 }}>{team.id}</span>
                          </div>
                        </td>
                        <td>
                          {team.api_key ? (
                            <code style={{
                              fontFamily: 'var(--font-mono)', fontSize: 12,
                              background: 'var(--surface-2)', border: '1px solid var(--border)',
                              borderRadius: 'var(--r-xs)', padding: '2px 8px',
                              letterSpacing: show ? undefined : '0.08em',
                            }}>
                              {maskedKey}
                            </code>
                          ) : <span className="muted">—</span>}
                        </td>
                        <td style={{ fontSize: 12, fontFamily: 'var(--font-mono)' }}>
                          {rl ? (
                            <span>
                              {rl.rpm != null ? `${rl.rpm} rpm` : ''}
                              {rl.rpm != null && rl.tpm != null ? ' / ' : ''}
                              {rl.tpm != null ? `${rl.tpm} tpm` : ''}
                            </span>
                          ) : <span className="muted">no limit</span>}
                        </td>
                        <td>
                          {(team.policy.allowed_models ?? []).length > 0 ? (
                            <div style={{ display: 'flex', gap: 4, flexWrap: 'wrap' }}>
                              {(team.policy.allowed_models ?? []).slice(0, 2).map((m) => (
                                <span key={m} className="badge mono" style={{ fontSize: 11 }}>{m}</span>
                              ))}
                              {(team.policy.allowed_models ?? []).length > 2 && (
                                <span className="badge">+{(team.policy.allowed_models ?? []).length - 2}</span>
                              )}
                            </div>
                          ) : <span className="muted" style={{ fontSize: 12 }}>all models</span>}
                        </td>
                        <td>
                          {team.api_key && (
                            <div style={{ display: 'flex', gap: 4 }}>
                              <button
                                className="btn btn-ghost btn-sm"
                                style={{ padding: '0 6px' }}
                                onClick={() => toggleReveal(team.id)}
                                title={show ? 'Hide' : 'Show'}
                              >
                                <Icon name={show ? 'eye-off' : 'eye'} size={13} />
                              </button>
                              <button
                                className="btn btn-ghost btn-sm"
                                style={{ padding: '0 6px' }}
                                onClick={() => copyKey(team.id, team.api_key)}
                                title="Copy"
                              >
                                <Icon name="copy" size={13} />
                              </button>
                            </div>
                          )}
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
