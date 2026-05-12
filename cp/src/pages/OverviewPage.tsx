import { useState } from 'react'
import { useQuery } from '@tanstack/react-query'
import Topbar from '../components/Topbar.tsx'
import Sparkline from '../components/Sparkline.tsx'
import Empty from '../components/Empty.tsx'
import Icon from '../components/Icon.tsx'
import { api } from '../lib/api.ts'
import type { TimeRange, TrendPoint, Overview } from '../lib/types.ts'

const RANGES: { label: string; value: TimeRange }[] = [
  { label: 'Last 1h', value: '1h' },
  { label: 'Last 24h', value: '24h' },
  { label: 'Last 7d', value: '7d' },
  { label: 'Last 30d', value: '30d' },
]

function fmt(n: number): string {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`
  if (n >= 1_000) return `${(n / 1_000).toFixed(1)}k`
  return String(n)
}

function fmtMs(ms: number): string {
  return ms >= 1000 ? `${(ms / 1000).toFixed(2)}s` : `${Math.round(ms)}ms`
}

interface StatCardProps {
  label: string
  value: string
  delta: number
  sub?: string
  series: number[]
  color?: string
}

function StatCard({ label, value, delta, sub, series, color = 'var(--brand)' }: StatCardProps) {
  const up = delta >= 0
  return (
    <div className="card" style={{ padding: '18px 20px', position: 'relative', overflow: 'hidden' }}>
      <div style={{ position: 'absolute', top: 16, right: 16, opacity: 0.7 }}>
        <Sparkline values={series} color={color} width={80} height={32} />
      </div>
      <div style={{ fontSize: 12, color: 'var(--muted)', marginBottom: 8 }}>{label}</div>
      <div style={{ fontSize: 24, fontWeight: 600, letterSpacing: '-0.02em', marginBottom: 4 }}>{value}</div>
      {sub && <div style={{ fontSize: 12, color: 'var(--muted)', marginBottom: 4 }}>{sub}</div>}
      <div style={{ fontSize: 12, color: up ? 'var(--ok)' : 'var(--err)' }}>
        {up ? '↑' : '↓'} {Math.abs(delta).toFixed(1)}%
      </div>
    </div>
  )
}

function TrendChart({ points }: { points: TrendPoint[] }) {
  if (!points.length) return null
  const maxReq = Math.max(...points.map((p) => p.requests), 1)
  const maxTok = Math.max(...points.map((p) => p.total_tokens), 1)
  const W = 960, H = 200, PAD = { top: 16, right: 40, bottom: 28, left: 40 }
  const inner = { w: W - PAD.left - PAD.right, h: H - PAD.top - PAD.bottom }
  const n = points.length

  const barW = Math.max(4, (inner.w / n) - 3)
  const bars = points.map((p, i) => ({
    x: PAD.left + (i / n) * inner.w,
    h: (p.requests / maxReq) * inner.h,
    label: p.label,
  }))

  const linePoints = points
    .map((p, i) => {
      const x = PAD.left + (i / (n - 1)) * inner.w
      const y = PAD.top + inner.h - (p.total_tokens / maxTok) * inner.h
      return `${x},${y}`
    })
    .join(' ')

  return (
    <div className="card" style={{ padding: '18px 20px', marginBottom: 16 }}>
      <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', marginBottom: 12 }}>
        <span style={{ fontWeight: 600, fontSize: 14 }}>Traffic Trend</span>
        <div style={{ display: 'flex', gap: 16, fontSize: 12, color: 'var(--muted)' }}>
          <span style={{ display: 'flex', alignItems: 'center', gap: 5 }}>
            <span style={{ width: 10, height: 10, background: 'var(--brand)', borderRadius: 2, display: 'inline-block' }} /> Requests
          </span>
          <span style={{ display: 'flex', alignItems: 'center', gap: 5 }}>
            <span style={{ width: 16, height: 2, background: 'oklch(0.65 0.15 60)', display: 'inline-block', borderRadius: 1 }} /> Tokens
          </span>
        </div>
      </div>
      <svg width="100%" viewBox={`0 0 ${W} ${H}`} style={{ overflow: 'visible' }}>
        {/* Gridlines */}
        {[0.25, 0.5, 0.75, 1].map((f) => (
          <line key={f}
            x1={PAD.left} y1={PAD.top + inner.h * (1 - f)}
            x2={PAD.left + inner.w} y2={PAD.top + inner.h * (1 - f)}
            stroke="var(--border)" strokeDasharray="4 4" />
        ))}
        {/* Bars */}
        {bars.map((b, i) => (
          <rect key={i}
            x={b.x} y={PAD.top + inner.h - b.h}
            width={barW} height={b.h}
            fill="var(--brand)" opacity={0.75} rx={2} />
        ))}
        {/* Token line */}
        {n > 1 && (
          <polyline points={linePoints} fill="none"
            stroke="oklch(0.65 0.15 60)" strokeWidth={2}
            strokeLinejoin="round" strokeLinecap="round" />
        )}
        {/* X labels */}
        {points.filter((_, i) => i % Math.ceil(n / 8) === 0).map((p, i, arr) => {
          const orig = points.indexOf(p)
          const x = PAD.left + (orig / n) * inner.w + barW / 2
          return (
            <text key={i} x={x} y={H - 4} textAnchor="middle"
              fontSize={10} fill="var(--muted)">{arr[i].label}</text>
          )
        })}
      </svg>
    </div>
  )
}

function TopologySection({ flows }: { flows: { team_id: string; router: string; channel: string; model: string; requests: number; total_tokens: number }[] }) {
  if (!flows.length) return null

  type Col = { header: string; items: { name: string; value: number }[] }
  const cols: Col[] = [
    { header: 'Teams', items: [...new Map(flows.map((f) => [f.team_id, f])).values()].map((f) => ({ name: f.team_id, value: f.requests })) },
    { header: 'Routers', items: [...new Map(flows.map((f) => [f.router, f])).values()].map((f) => ({ name: f.router, value: f.requests })) },
    { header: 'Channels', items: [...new Map(flows.map((f) => [f.channel, f])).values()].map((f) => ({ name: f.channel, value: f.requests })) },
    { header: 'Models', items: [...new Map(flows.map((f) => [f.model, f])).values()].map((f) => ({ name: f.model, value: f.requests })) },
  ]
  const allMax = Math.max(...cols.flatMap((c) => c.items.map((i) => i.value)), 1)

  return (
    <div className="card" style={{ marginBottom: 16 }}>
      <div style={{ padding: '14px 20px', borderBottom: '1px solid var(--border)', fontWeight: 600, fontSize: 14 }}>
        Traffic Topology
      </div>
      <div style={{ display: 'grid', gridTemplateColumns: 'repeat(4, 1fr)', padding: '16px 0' }}>
        {cols.map((col, ci) => (
          <div key={col.header} style={{
            borderRight: ci < 3 ? '1px solid var(--border)' : undefined,
            padding: '0 16px',
          }}>
            <div style={{ fontSize: 11, textTransform: 'uppercase', letterSpacing: '0.06em', color: 'var(--muted)', fontWeight: 500, marginBottom: 10 }}>
              {col.header}
            </div>
            {col.items.slice(0, 4).map((item) => (
              <div key={item.name} className="card" style={{
                padding: '10px 12px', marginBottom: 8, position: 'relative', overflow: 'hidden',
              }}>
                <div style={{ fontSize: 13, fontWeight: 500, marginBottom: 2, whiteSpace: 'nowrap', overflow: 'hidden', textOverflow: 'ellipsis' }}>
                  {item.name}
                </div>
                <div style={{ fontSize: 11, fontFamily: 'var(--font-mono)', color: 'var(--muted)' }}>{fmt(item.value)} req</div>
                <div style={{
                  position: 'absolute', bottom: 0, left: 0,
                  height: 3, borderRadius: '0 2px 0 0',
                  background: 'var(--brand)',
                  width: `${(item.value / allMax) * 100}%`,
                }} />
              </div>
            ))}
          </div>
        ))}
      </div>
    </div>
  )
}

function overviewSeries(overview: Overview): { req: number[]; tok: number[]; lat: number[]; ok: number[] } {
  // Backend doesn't return per-card sparkline series; generate flat lines as fallback
  return {
    req: [overview.total_requests],
    tok: [overview.total_tokens],
    lat: [overview.avg_latency_ms],
    ok: [overview.success_rate * 100],
  }
}

export default function OverviewPage() {
  const [range, setRange] = useState<TimeRange>('24h')

  const { data, isLoading, error } = useQuery({
    queryKey: ['analytics', range],
    queryFn: () => api.analytics({ range }),
  })

  const actions = (
    <div style={{ display: 'flex', gap: 8 }}>
      <select
        className="select btn-sm"
        value={range}
        onChange={(e) => setRange(e.target.value as TimeRange)}
        style={{ height: 28, fontSize: 12 }}
      >
        {RANGES.map((r) => (
          <option key={r.value} value={r.value}>{r.label}</option>
        ))}
      </select>
    </div>
  )

  return (
    <>
      <Topbar breadcrumbs={[{ label: 'Operate' }, { label: 'Overview' }]} actions={actions} />
      <div className="page-pad">
        <div className="page-head">
          <h1 className="page-title">Dashboard</h1>
          <p className="page-sub">Real-time gateway health across all teams, routers, and channels.</p>
        </div>

        {isLoading && (
          <div style={{ display: 'flex', justifyContent: 'center', padding: 48 }}>
            <span className="spinner" style={{ width: 24, height: 24 }} />
          </div>
        )}

        {error && (
          <div style={{ padding: '12px 16px', background: 'var(--err-soft)', color: 'var(--err)', borderRadius: 'var(--r-md)', fontSize: 13 }}>
            Failed to load analytics. {error instanceof Error ? error.message : 'Unknown error'}
          </div>
        )}

        {data && (() => {
          const ov = data.overview
          void overviewSeries(ov)
          const trendPts = data.trend.points
          return (
            <>
              {/* Stat cards */}
              <div style={{ display: 'grid', gridTemplateColumns: 'repeat(4, 1fr)', gap: 16, marginBottom: 16 }}>
                <StatCard
                  label="Total Requests"
                  value={fmt(ov.total_requests)}
                  delta={ov.delta.total_requests}
                  series={trendPts.map((p) => p.requests)}
                />
                <StatCard
                  label="Total Tokens"
                  value={fmt(ov.total_tokens)}
                  delta={ov.delta.total_tokens}
                  sub={`In: ${fmt(ov.input_tokens)} · Out: ${fmt(ov.output_tokens)}`}
                  series={trendPts.map((p) => p.total_tokens)}
                  color="oklch(0.65 0.15 60)"
                />
                <StatCard
                  label="Avg Latency"
                  value={fmtMs(ov.avg_latency_ms)}
                  delta={-ov.delta.avg_latency_ms}
                  series={trendPts.map((p) => p.avg_latency_ms)}
                  color="oklch(0.6 0.1 230)"
                />
                <StatCard
                  label="Success Rate"
                  value={`${(ov.success_rate * 100).toFixed(1)}%`}
                  delta={ov.delta.success_rate}
                  series={trendPts.map((p) => p.success_rate * 100)}
                  color="var(--ok)"
                />
              </div>

              {/* Trend chart */}
              {trendPts.length > 0 && <TrendChart points={trendPts} />}

              {/* Topology */}
              <TopologySection flows={data.topology.flows} />

              {/* Model + Router breakdown */}
              {(data.model_router.model_share.length > 0 || data.system_reliability.channel_latency.length > 0) && (
                <div style={{ display: 'grid', gridTemplateColumns: '1.4fr 1fr', gap: 16, marginBottom: 16 }}>
                  {/* Channel health */}
                  <div className="card">
                    <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', padding: '14px 20px', borderBottom: '1px solid var(--border)' }}>
                      <span style={{ fontWeight: 600, fontSize: 14 }}>Channel Health</span>
                      <Icon name="refresh" size={14} style={{ color: 'var(--muted)' }} />
                    </div>
                    <table className="table">
                      <thead>
                        <tr>
                          <th>Channel</th>
                          <th style={{ textAlign: 'right' }}>Requests</th>
                          <th style={{ textAlign: 'right' }}>Avg</th>
                          <th style={{ textAlign: 'right' }}>p95</th>
                        </tr>
                      </thead>
                      <tbody>
                        {data.system_reliability.channel_latency.slice(0, 6).map((ch) => (
                          <tr key={ch.channel} className="row-hover">
                            <td style={{ fontWeight: 500 }}>{ch.channel}</td>
                            <td style={{ textAlign: 'right', fontFamily: 'var(--font-mono)', fontSize: 12 }}>{fmt(ch.total_requests)}</td>
                            <td style={{ textAlign: 'right', fontFamily: 'var(--font-mono)', fontSize: 12, color: ch.avg_latency_ms > 1000 ? 'var(--warn)' : undefined }}>{fmtMs(ch.avg_latency_ms)}</td>
                            <td style={{ textAlign: 'right', fontFamily: 'var(--font-mono)', fontSize: 12, color: ch.p95_latency_ms > 1000 ? 'var(--warn)' : undefined }}>{fmtMs(ch.p95_latency_ms)}</td>
                          </tr>
                        ))}
                      </tbody>
                    </table>
                    {data.system_reliability.channel_latency.length === 0 && (
                      <Empty icon="plug" title="No channel data yet" />
                    )}
                  </div>

                  {/* Model share */}
                  <div className="card">
                    <div style={{ padding: '14px 20px', borderBottom: '1px solid var(--border)', fontWeight: 600, fontSize: 14 }}>
                      Model Usage
                    </div>
                    <div style={{ padding: '8px 0' }}>
                      {data.model_router.model_share.slice(0, 6).map((m) => (
                        <div key={m.name} style={{ padding: '8px 20px', display: 'flex', alignItems: 'center', gap: 10 }}>
                          <div style={{ flex: 1, fontSize: 13, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>{m.name}</div>
                          <div style={{ fontSize: 12, fontFamily: 'var(--font-mono)', color: 'var(--muted)', width: 40, textAlign: 'right' }}>
                            {m.percentage.toFixed(0)}%
                          </div>
                          <div style={{ width: 80, height: 4, background: 'var(--surface-2)', borderRadius: 2, overflow: 'hidden' }}>
                            <div style={{ width: `${m.percentage}%`, height: '100%', background: 'var(--brand)', borderRadius: 2 }} />
                          </div>
                        </div>
                      ))}
                      {data.model_router.model_share.length === 0 && (
                        <Empty icon="cube" title="No model data yet" />
                      )}
                    </div>
                  </div>
                </div>
              )}

              {!trendPts.length && !data.topology.flows.length && (
                <Empty icon="activity" title="No data for this period" sub="Requests will appear here as they flow through the gateway." />
              )}
            </>
          )
        })()}
      </div>
    </>
  )
}
