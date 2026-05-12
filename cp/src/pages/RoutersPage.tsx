import { useState } from 'react'
import { useQuery } from '@tanstack/react-query'
import Topbar from '../components/Topbar.tsx'
import Empty from '../components/Empty.tsx'
import { api } from '../lib/api.ts'
import type { AdminRouter } from '../lib/types.ts'

function RouterDetail({ router }: { router: AdminRouter }) {
  const channels = router.rules.flatMap((r) => r.channels).length > 0
    ? router.rules.flatMap((r) => r.channels)
    : (router.channels ?? [])

  const strategy = router.rules[0]?.strategy ?? router.strategy ?? 'round_robin'

  return (
    <div style={{ flex: 1 }}>
      <div className="card" style={{ padding: '18px 20px' }}>
        <div style={{ marginBottom: 20 }}>
          <div style={{ fontSize: 16, fontWeight: 600 }}>{router.name}</div>
          <div style={{ fontSize: 12, fontFamily: 'var(--font-mono)', color: 'var(--muted)', marginTop: 2 }}>{router.name}</div>
        </div>

        {/* Strategy */}
        <div className="section-title">Strategy</div>
        <div style={{ display: 'flex', gap: 10, marginBottom: 20 }}>
          {['round_robin', 'weighted', 'priority'].map((s) => (
            <div key={s} className="card" style={{
              padding: '10px 16px', fontSize: 13, fontWeight: 500,
              borderColor: strategy === s ? 'var(--brand)' : undefined,
              background: strategy === s ? 'var(--brand-soft)' : undefined,
              color: strategy === s ? 'var(--brand-ink)' : undefined,
            }}>
              {s.replace('_', '-')}
            </div>
          ))}
        </div>

        {/* Channels */}
        <div className="section-title">Channels ({channels.length})</div>
        {channels.length === 0 ? (
          <div style={{ color: 'var(--muted)', fontSize: 13 }}>No channels assigned.</div>
        ) : (
          <div style={{ display: 'flex', flexDirection: 'column', gap: 8 }}>
            {channels.map((ch, i) => (
              <div key={`${ch.name}-${i}`} className="card" style={{ padding: '10px 14px', display: 'flex', alignItems: 'center', gap: 12 }}>
                <span style={{ width: 20, height: 20, borderRadius: 'var(--r-xs)', background: 'var(--surface-2)', border: '1px solid var(--border)', display: 'flex', alignItems: 'center', justifyContent: 'center', fontSize: 11, fontWeight: 600, color: 'var(--muted)', flexShrink: 0 }}>
                  {i + 1}
                </span>
                <span style={{ flex: 1, fontSize: 13, fontWeight: 500 }}>{ch.name}</span>
                {strategy === 'weighted' && (
                  <span style={{ fontSize: 12, fontFamily: 'var(--font-mono)', color: 'var(--muted)' }}>weight: {ch.weight}</span>
                )}
              </div>
            ))}
          </div>
        )}

        {/* Fallback */}
        {(router.fallback_channels ?? []).length > 0 && (
          <>
            <div className="section-title" style={{ marginTop: 20 }}>Fallback channels</div>
            <div style={{ display: 'flex', gap: 8, flexWrap: 'wrap' }}>
              {(router.fallback_channels ?? []).map((c) => (
                <span key={c} className="badge">{c}</span>
              ))}
            </div>
          </>
        )}

        {/* Rules */}
        {router.rules.length > 0 && (
          <>
            <div className="section-title" style={{ marginTop: 20 }}>Model rules ({router.rules.length})</div>
            <div style={{ display: 'flex', flexDirection: 'column', gap: 8 }}>
              {router.rules.map((rule, i) => (
                <div key={i} className="card" style={{ padding: '10px 14px', fontSize: 13 }}>
                  <div style={{ fontFamily: 'var(--font-mono)', fontSize: 12, color: 'var(--brand-ink)', marginBottom: 4 }}>
                    {rule.match.models.length > 0 ? rule.match.models.join(', ') : '*'}
                  </div>
                  <div style={{ display: 'flex', gap: 8, flexWrap: 'wrap' }}>
                    {rule.channels.map((ch) => (
                      <span key={ch.name} className="badge">{ch.name}</span>
                    ))}
                  </div>
                </div>
              ))}
            </div>
          </>
        )}
      </div>
    </div>
  )
}

export default function RoutersPage() {
  const { data, isLoading, error } = useQuery({
    queryKey: ['routers'],
    queryFn: api.routers,
  })

  const routers = data?.data ?? []
  const [selected, setSelected] = useState<string | null>(null)
  const active = routers.find((r) => r.name === selected) ?? routers[0] ?? null

  return (
    <>
      <Topbar breadcrumbs={[{ label: 'Configure' }, { label: 'Routers' }]} />
      <div className="page-pad">
        <div className="page-head">
          <h1 className="page-title">Routers</h1>
          <p className="page-sub">Define how requests are distributed across channels and models.</p>
        </div>

        {isLoading && (
          <div style={{ display: 'flex', justifyContent: 'center', padding: 48 }}>
            <span className="spinner" style={{ width: 20, height: 20 }} />
          </div>
        )}

        {error && (
          <div style={{ padding: '12px 16px', background: 'var(--err-soft)', color: 'var(--err)', borderRadius: 'var(--r-md)', fontSize: 13 }}>
            Failed to load routers.
          </div>
        )}

        {!isLoading && !error && routers.length === 0 && (
          <Empty icon="route" title="No routers configured" sub="Add routers to your config.json to define load balancing rules." />
        )}

        {routers.length > 0 && (
          <div style={{ display: 'grid', gridTemplateColumns: '280px 1fr', gap: 16 }}>
            {/* Router list */}
            <div className="card" style={{ padding: 8, alignSelf: 'start' }}>
              {routers.map((r) => {
                const isActive = (active?.name ?? '') === r.name
                const channels = r.rules.flatMap((ru) => ru.channels).length || (r.channels ?? []).length
                return (
                  <button
                    key={r.name}
                    onClick={() => setSelected(r.name)}
                    style={{
                      display: 'block', width: '100%', textAlign: 'left',
                      padding: '10px 12px', borderRadius: 'var(--r-sm)',
                      background: isActive ? 'var(--bg-soft)' : 'transparent',
                      border: isActive ? '1px solid var(--border)' : '1px solid transparent',
                      cursor: 'pointer', marginBottom: 2,
                    }}
                  >
                    <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
                      <span className={`dot dot-${isActive ? 'ok' : 'muted'}`} />
                      <span style={{ fontWeight: 500, fontSize: 13 }}>{r.name}</span>
                    </div>
                    <div style={{ fontSize: 12, color: 'var(--muted)', marginTop: 2, fontFamily: 'var(--font-mono)' }}>
                      {r.strategy ?? 'round_robin'} · {channels} ch
                    </div>
                  </button>
                )
              })}
            </div>

            {/* Detail */}
            {active && <RouterDetail router={active} />}
          </div>
        )}
      </div>
    </>
  )
}
