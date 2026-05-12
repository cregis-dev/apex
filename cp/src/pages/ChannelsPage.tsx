import { useQuery } from '@tanstack/react-query'
import Topbar from '../components/Topbar.tsx'
import ProviderMark from '../components/ProviderMark.tsx'
import Icon from '../components/Icon.tsx'
import Empty from '../components/Empty.tsx'
import { api } from '../lib/api.ts'

export default function ChannelsPage() {
  const { data, isLoading, error, refetch } = useQuery({
    queryKey: ['channels'],
    queryFn: api.channels,
  })

  const channels = data?.data ?? []

  return (
    <>
      <Topbar
        breadcrumbs={[{ label: 'Configure' }, { label: 'Channels' }]}
        actions={
          <>
            <button className="btn btn-sm" onClick={() => void refetch()}>
              <Icon name="refresh" size={13} /> Refresh
            </button>
          </>
        }
      />
      <div className="page-pad">
        <div className="page-head">
          <h1 className="page-title">Channels</h1>
          <p className="page-sub">Upstream provider connections. Each channel maps to one provider account or endpoint.</p>
        </div>

        {isLoading && (
          <div style={{ display: 'flex', justifyContent: 'center', padding: 48 }}>
            <span className="spinner" style={{ width: 20, height: 20 }} />
          </div>
        )}

        {error && (
          <div style={{ padding: '12px 16px', background: 'var(--err-soft)', color: 'var(--err)', borderRadius: 'var(--r-md)', fontSize: 13 }}>
            Failed to load channels. {error instanceof Error ? error.message : ''}
          </div>
        )}

        {!isLoading && !error && (
          <>
            {/* Summary strip */}
            {channels.length > 0 && (
              <div style={{ display: 'flex', gap: 12, marginBottom: 16 }}>
                {[
                  { label: 'Total', value: channels.length, color: 'var(--ink)' },
                ].map((s) => (
                  <div key={s.label} className="card" style={{ padding: '12px 20px', display: 'flex', alignItems: 'baseline', gap: 8 }}>
                    <span style={{ fontSize: 22, fontWeight: 600, color: s.color }}>{s.value}</span>
                    <span style={{ fontSize: 13, color: 'var(--muted)' }}>{s.label}</span>
                  </div>
                ))}
              </div>
            )}

            <div className="card">
              {channels.length === 0 ? (
                <Empty icon="plug" title="No channels configured" sub="Add channels to your config.json to connect upstream LLM providers." />
              ) : (
                <table className="table">
                  <thead>
                    <tr>
                      <th>Channel</th>
                      <th>Provider</th>
                      <th>Base URL</th>
                      <th>API Key</th>
                    </tr>
                  </thead>
                  <tbody>
                    {channels.map((ch) => (
                      <tr key={ch.name} className="row-hover">
                        <td>
                          <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
                            <ProviderMark kind={ch.provider_type} size={28} />
                            <span style={{ fontWeight: 500 }}>{ch.name}</span>
                          </div>
                        </td>
                        <td>
                          <span className="badge">{ch.provider_type}</span>
                        </td>
                        <td style={{ fontFamily: 'var(--font-mono)', fontSize: 12, color: 'var(--muted)' }}>
                          {ch.base_url || '—'}
                        </td>
                        <td style={{ fontFamily: 'var(--font-mono)', fontSize: 12, color: 'var(--muted)' }}>
                          {ch.api_key || '—'}
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              )}
            </div>
          </>
        )}
      </div>
    </>
  )
}
