import { useQuery } from '@tanstack/react-query'
import Topbar from '../components/Topbar.tsx'
import Empty from '../components/Empty.tsx'
import MiniBars from '../components/MiniBars.tsx'
import StatusPill from '../components/StatusPill.tsx'
import { api } from '../lib/api.ts'

interface ModelRow {
  alias: string
  routers: string[]
  channels: string[]
  strategy: string
}

export default function ModelsPage() {
  const { data: routersData, isLoading, error } = useQuery({
    queryKey: ['routers'],
    queryFn: api.routers,
  })

  const { data: analyticsData } = useQuery({
    queryKey: ['analytics', '24h'],
    queryFn: () => api.analytics({ range: '24h' }),
  })

  const routers = routersData?.data ?? []

  // Derive model rows from router rules
  const modelMap = new Map<string, ModelRow>()
  routers.forEach((router) => {
    router.rules.forEach((rule) => {
      rule.match.models.forEach((model) => {
        const existing = modelMap.get(model)
        const channels = rule.channels.map((c) => c.name)
        if (existing) {
          if (!existing.routers.includes(router.name)) existing.routers.push(router.name)
          channels.forEach((c) => { if (!existing.channels.includes(c)) existing.channels.push(c) })
        } else {
          modelMap.set(model, {
            alias: model,
            routers: [router.name],
            channels,
            strategy: rule.strategy,
          })
        }
      })
    })
    // Also handle legacy-style direct channels
    if ((router.channels ?? []).length > 0 && router.rules.length === 0) {
      const key = `*@${router.name}`
      modelMap.set(key, {
        alias: `* (${router.name})`,
        routers: [router.name],
        channels: (router.channels ?? []).map((c) => c.name),
        strategy: router.strategy ?? 'round_robin',
      })
    }
  })

  const modelRows = [...modelMap.values()]

  // Usage per model from analytics
  const modelShare = analyticsData?.model_router.model_share ?? []
  const usageMap = new Map(modelShare.map((m) => [m.name, m]))
  const maxReq = Math.max(...modelShare.map((m) => m.requests), 1)

  const sparkData = (name: string) => {
    const pts = analyticsData?.trend.points ?? []
    // Approximate: use overall trend shape scaled by model share
    const share = usageMap.get(name)?.percentage ?? 0
    return pts.map((p) => p.requests * share / 100)
  }

  return (
    <>
      <Topbar breadcrumbs={[{ label: 'Configure' }, { label: 'Models' }]} />
      <div className="page-pad">
        <div className="page-head">
          <h1 className="page-title">Models</h1>
          <p className="page-sub">Aliases, mappings, and per-model usage. Defined via router rules in the config.</p>
        </div>

        {isLoading && (
          <div style={{ display: 'flex', justifyContent: 'center', padding: 48 }}>
            <span className="spinner" style={{ width: 20, height: 20 }} />
          </div>
        )}

        {error && (
          <div style={{ padding: '12px 16px', background: 'var(--err-soft)', color: 'var(--err)', borderRadius: 'var(--r-md)', fontSize: 13 }}>
            Failed to load model data.
          </div>
        )}

        {!isLoading && !error && modelRows.length === 0 && (
          <Empty
            icon="cube"
            title="No model rules configured"
            sub="Add router rules with model match patterns to see them here."
          />
        )}

        {modelRows.length > 0 && (
          <div className="card">
            <table className="table">
              <thead>
                <tr>
                  <th>Model / Pattern</th>
                  <th>Routers</th>
                  <th>Channels</th>
                  <th>Strategy</th>
                  <th style={{ textAlign: 'right' }}>Req (24h)</th>
                  <th style={{ width: 130 }}>Trend</th>
                  <th>Status</th>
                </tr>
              </thead>
              <tbody>
                {modelRows.map((row) => {
                  const usage = usageMap.get(row.alias)
                  const reqs = usage?.requests ?? 0
                  const pct = usage?.percentage ?? 0
                  const sd = sparkData(row.alias)
                  return (
                    <tr key={row.alias} className="row-hover">
                      <td>
                        <span style={{ fontFamily: 'var(--font-mono)', fontWeight: 600, fontSize: 13 }}>
                          {row.alias}
                        </span>
                      </td>
                      <td>
                        <div style={{ display: 'flex', gap: 4, flexWrap: 'wrap' }}>
                          {row.routers.map((r) => (
                            <span key={r} className="badge">{r}</span>
                          ))}
                        </div>
                      </td>
                      <td>
                        <div style={{ display: 'flex', gap: 4, flexWrap: 'wrap' }}>
                          {row.channels.slice(0, 2).map((c) => (
                            <span key={c} className="badge mono" style={{ fontSize: 11 }}>{c}</span>
                          ))}
                          {row.channels.length > 2 && <span className="badge">+{row.channels.length - 2}</span>}
                        </div>
                      </td>
                      <td>
                        <span className="badge">{row.strategy.replace('_', '-')}</span>
                      </td>
                      <td style={{ textAlign: 'right', fontFamily: 'var(--font-mono)', fontSize: 12 }}>
                        {reqs > 0 ? (
                          <span title={`${pct.toFixed(1)}% of traffic`}>
                            {reqs.toLocaleString()}
                          </span>
                        ) : <span className="muted">—</span>}
                      </td>
                      <td>
                        {sd.some((v) => v > 0)
                          ? <MiniBars values={sd} width={120} height={24} />
                          : <span className="muted" style={{ fontSize: 12 }}>no data</span>}
                      </td>
                      <td>
                        <StatusPill
                          status={reqs > 0 ? 'ok' : 'info'}
                          label={reqs > 0 ? 'Active' : 'Idle'}
                        />
                      </td>
                    </tr>
                  )
                })}
              </tbody>
            </table>
          </div>
        )}

        {/* Also show models seen in analytics but not in config */}
        {modelShare.filter((m) => !modelMap.has(m.name)).length > 0 && (
          <div style={{ marginTop: 16 }}>
            <div className="section-title">Models seen in traffic (no explicit rule)</div>
            <div className="card" style={{ padding: '12px 20px' }}>
              <div style={{ display: 'flex', gap: 8, flexWrap: 'wrap' }}>
                {modelShare
                  .filter((m) => !modelMap.has(m.name))
                  .map((m) => (
                    <div key={m.name} style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
                      <span className="badge mono" style={{ fontSize: 11 }}>{m.name}</span>
                      <div style={{ width: 40, height: 3, background: 'var(--surface-2)', borderRadius: 2, overflow: 'hidden' }}>
                        <div style={{ width: `${(m.requests / maxReq) * 100}%`, height: '100%', background: 'var(--muted-2)', borderRadius: 2 }} />
                      </div>
                      <span style={{ fontSize: 11, color: 'var(--muted)' }}>{m.requests.toLocaleString()}</span>
                    </div>
                  ))}
              </div>
            </div>
          </div>
        )}
      </div>
    </>
  )
}
