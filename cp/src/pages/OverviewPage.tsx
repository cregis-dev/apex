import { useState } from 'react'
import { useQuery } from '@tanstack/react-query'
import Topbar from '../components/Topbar.tsx'
import Sparkline from '../components/Sparkline.tsx'
import Empty from '../components/Empty.tsx'
import Icon from '../components/Icon.tsx'
import FiltersBar, { DEFAULT_FILTERS, filterValuesToParams, type FilterValues } from '../components/FiltersBar.tsx'
import { api } from '../lib/api.ts'
import type { TrendPoint, TopologyNode, TopologyLink, FlowSummary } from '../lib/types.ts'

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

const KIND_ORDER = ['team', 'router', 'channel', 'model'] as const
const COLUMN_HEADERS = ['TEAMS', 'ROUTERS', 'CHANNELS', 'MODELS'] as const
const KIND_FILL: Record<string, string> = {
  team: 'oklch(0.66 0.14 55)',
  router: 'oklch(0.62 0.14 48)',
  channel: 'oklch(0.58 0.13 42)',
  model: 'oklch(0.54 0.12 38)',
}
function depthOf(kind: string): number {
  const i = KIND_ORDER.indexOf(kind as (typeof KIND_ORDER)[number])
  return i < 0 ? 0 : i
}

/** Hand-drawn layered Sankey: team → router → channel → model. */
function SankeyDiagram({ nodes, links }: { nodes: TopologyNode[]; links: TopologyLink[] }) {
  const n = nodes.length
  const W = 960
  const NODE_W = 12
  const PAD = { top: 28, right: 18, bottom: 14, left: 18 }
  const GAP = 16

  // Throughput per node = max(in, out) so source/sink nodes size correctly.
  const inSum = new Array(n).fill(0)
  const outSum = new Array(n).fill(0)
  for (const l of links) {
    outSum[l.source] += l.value
    inSum[l.target] += l.value
  }
  const valOf = (i: number) => Math.max(inSum[i], outSum[i], 0)

  // Group node indices into the 4 layers.
  const layers: number[][] = [[], [], [], []]
  nodes.forEach((nd, i) => layers[depthOf(nd.kind)].push(i))

  const layerTotals = layers.map((L) => L.reduce((s, i) => s + valOf(i), 0))
  const maxTotal = Math.max(...layerTotals, 1)
  const maxCount = Math.max(...layers.map((L) => L.length), 1)

  // Height: comfortable per-unit scale, clamped.
  const baseUnit = 24
  let innerH = maxTotal * baseUnit + GAP * (maxCount - 1)
  innerH = Math.max(200, Math.min(440, innerH))
  const scale = (innerH - GAP * (maxCount - 1)) / maxTotal
  const H = innerH + PAD.top + PAD.bottom

  const colX = (depth: number) =>
    PAD.left + (W - PAD.left - PAD.right - NODE_W) * (depth / (KIND_ORDER.length - 1))

  // Position every node within its layer (tallest first for a tidy stack).
  const pos = new Map<number, { x: number; y: number; h: number; depth: number }>()
  layers.forEach((L, depth) => {
    const total = layerTotals[depth]
    const layerH = total * scale + GAP * (Math.max(L.length, 1) - 1)
    let y = PAD.top + (innerH - layerH) / 2
    const sorted = [...L].sort((a, b) => valOf(b) - valOf(a) || nodes[a].name.localeCompare(nodes[b].name))
    for (const i of sorted) {
      const h = Math.max(6, valOf(i) * scale)
      pos.set(i, { x: colX(depth), y, h, depth })
      y += h + GAP
    }
  })

  // Assign each link a vertical sub-band on both ends, ordered to reduce crossings.
  const sourceMid = new Map<number, number>()
  const targetMid = new Map<number, number>()
  const bySource = new Map<number, number[]>()
  const byTarget = new Map<number, number[]>()
  links.forEach((l, idx) => {
    ;(bySource.get(l.source) ?? bySource.set(l.source, []).get(l.source)!).push(idx)
    ;(byTarget.get(l.target) ?? byTarget.set(l.target, []).get(l.target)!).push(idx)
  })
  for (const [s, arr] of bySource) {
    arr.sort((a, b) => (pos.get(links[a].target)?.y ?? 0) - (pos.get(links[b].target)?.y ?? 0))
    let cy = pos.get(s)?.y ?? 0
    for (const idx of arr) {
      const w = links[idx].value * scale
      sourceMid.set(idx, cy + w / 2)
      cy += w
    }
  }
  for (const [t, arr] of byTarget) {
    arr.sort((a, b) => (pos.get(links[a].source)?.y ?? 0) - (pos.get(links[b].source)?.y ?? 0))
    let cy = pos.get(t)?.y ?? 0
    for (const idx of arr) {
      const w = links[idx].value * scale
      targetMid.set(idx, cy + w / 2)
      cy += w
    }
  }

  const totalReq = Math.max(...layerTotals, 1)

  return (
    <svg width="100%" viewBox={`0 0 ${W} ${H}`} style={{ display: 'block' }}>
      {/* Column headers */}
      {COLUMN_HEADERS.map((label, depth) => (
        <text
          key={label}
          x={colX(depth) + NODE_W / 2}
          y={16}
          textAnchor={depth === 0 ? 'start' : depth === 3 ? 'end' : 'middle'}
          fontSize={10.5}
          fontWeight={600}
          letterSpacing="0.06em"
          fill="var(--muted)"
        >
          {label}
        </text>
      ))}

      {/* Ribbons */}
      {links.map((l, idx) => {
        const sp = pos.get(l.source)
        const tp = pos.get(l.target)
        if (!sp || !tp) return null
        const x0 = sp.x + NODE_W
        const y0 = sourceMid.get(idx) ?? sp.y
        const x1 = tp.x
        const y1 = targetMid.get(idx) ?? tp.y
        const mx = (x0 + x1) / 2
        const width = Math.max(1, l.value * scale)
        const share = l.value / totalReq
        const opacity = Math.max(0.18, Math.min(0.5, 0.16 + share * 0.7))
        return (
          <path
            key={idx}
            d={`M${x0},${y0} C${mx},${y0} ${mx},${y1} ${x1},${y1}`}
            fill="none"
            stroke="var(--brand)"
            strokeOpacity={opacity}
            strokeWidth={width}
          >
            <title>
              {`${nodes[l.source].name} → ${nodes[l.target].name}\n${fmt(l.value)} req · ${fmt(l.total_tokens)} tokens`}
            </title>
          </path>
        )
      })}

      {/* Nodes + labels */}
      {nodes.map((nd, i) => {
        const p = pos.get(i)
        if (!p) return null
        const labelLeft = p.depth === 3
        const lx = labelLeft ? p.x - 6 : p.x + NODE_W + 6
        const cy = p.y + p.h / 2
        return (
          <g key={i}>
            <rect
              x={p.x}
              y={p.y}
              width={NODE_W}
              height={p.h}
              rx={2}
              fill={KIND_FILL[nd.kind] ?? 'var(--brand)'}
            >
              <title>{`${nd.name}\n${fmt(valOf(i))} req`}</title>
            </rect>
            <text
              x={lx}
              y={cy - 4}
              textAnchor={labelLeft ? 'end' : 'start'}
              fontSize={12}
              fontWeight={500}
              fill="var(--ink)"
              style={{ paintOrder: 'stroke', stroke: 'var(--surface)', strokeWidth: 3 }}
            >
              {nd.name}
            </text>
            <text
              x={lx}
              y={cy + 9}
              textAnchor={labelLeft ? 'end' : 'start'}
              fontSize={10}
              fontFamily="var(--font-mono)"
              fill="var(--muted)"
              style={{ paintOrder: 'stroke', stroke: 'var(--surface)', strokeWidth: 3 }}
            >
              {`${fmt(valOf(i))} req`}
            </text>
          </g>
        )
      })}
    </svg>
  )
}

/** Compact fallback: 4 columns of cards. Good for very dense graphs. */
function CompactTopology({ flows }: { flows: FlowSummary[] }) {
  type Col = { header: string; items: { name: string; value: number }[] }
  const dedupe = (pairs: [string, number][]) => {
    const m = new Map<string, number>()
    for (const [name, v] of pairs) m.set(name, (m.get(name) ?? 0) + v)
    return [...m.entries()].sort((a, b) => b[1] - a[1]).map(([name, value]) => ({ name, value }))
  }
  const cols: Col[] = [
    { header: 'Teams', items: dedupe(flows.map((f) => [f.team_id, f.requests])) },
    { header: 'Routers', items: dedupe(flows.map((f) => [f.router, f.requests])) },
    { header: 'Channels', items: dedupe(flows.map((f) => [f.channel, f.requests])) },
    { header: 'Models', items: dedupe(flows.map((f) => [f.model, f.requests])) },
  ]
  const allMax = Math.max(...cols.flatMap((c) => c.items.map((i) => i.value)), 1)

  return (
    <div style={{ display: 'grid', gridTemplateColumns: 'repeat(4, 1fr)', padding: '16px 0' }}>
      {cols.map((col, ci) => (
        <div key={col.header} style={{ borderRight: ci < 3 ? '1px solid var(--border)' : undefined, padding: '0 16px' }}>
          <div style={{ fontSize: 11, textTransform: 'uppercase', letterSpacing: '0.06em', color: 'var(--muted)', fontWeight: 500, marginBottom: 10 }}>
            {col.header}
          </div>
          {col.items.slice(0, 6).map((item) => (
            <div key={item.name} className="card" style={{ padding: '10px 12px', marginBottom: 8, position: 'relative', overflow: 'hidden' }}>
              <div style={{ fontSize: 13, fontWeight: 500, marginBottom: 2, whiteSpace: 'nowrap', overflow: 'hidden', textOverflow: 'ellipsis' }}>
                {item.name}
              </div>
              <div style={{ fontSize: 11, fontFamily: 'var(--font-mono)', color: 'var(--muted)' }}>{fmt(item.value)} req</div>
              <div style={{ position: 'absolute', bottom: 0, left: 0, height: 3, borderRadius: '0 2px 0 0', background: 'var(--brand)', width: `${(item.value / allMax) * 100}%` }} />
            </div>
          ))}
        </div>
      ))}
    </div>
  )
}

function TopologySection({
  topology,
}: {
  topology: { nodes: TopologyNode[]; links: TopologyLink[]; flows: FlowSummary[]; render_mode: string }
}) {
  const hasGraph = topology.nodes.length > 0 && topology.links.length > 0
  // Default to the compact card view when the backend flags a collapsed graph.
  const [view, setView] = useState<'sankey' | 'compact'>(
    topology.render_mode === 'summary' ? 'compact' : 'sankey'
  )

  if (!hasGraph && !topology.flows.length) return null

  const TabButton = ({ value, label }: { value: 'sankey' | 'compact'; label: string }) => (
    <button
      type="button"
      onClick={() => setView(value)}
      className="btn btn-sm"
      style={{
        height: 26, fontSize: 12,
        background: view === value ? 'var(--brand-soft)' : 'transparent',
        color: view === value ? 'var(--brand-ink)' : 'var(--muted)',
        borderColor: view === value ? 'transparent' : 'var(--border)',
      }}
    >
      {label}
    </button>
  )

  return (
    <div className="card" style={{ marginBottom: 16 }}>
      <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', padding: '14px 20px', borderBottom: '1px solid var(--border)' }}>
        <span style={{ fontWeight: 600, fontSize: 14 }}>Traffic Topology</span>
        {hasGraph && (
          <div style={{ display: 'flex', gap: 6 }}>
            <TabButton value="sankey" label="Sankey" />
            <TabButton value="compact" label="Compact" />
          </div>
        )}
      </div>
      {view === 'sankey' && hasGraph ? (
        <div style={{ padding: '12px 16px' }}>
          <SankeyDiagram nodes={topology.nodes} links={topology.links} />
        </div>
      ) : (
        <CompactTopology flows={topology.flows} />
      )}
    </div>
  )
}

export default function OverviewPage() {
  const [filters, setFilters] = useState<FilterValues>(DEFAULT_FILTERS)

  const params = filterValuesToParams(filters)
  const { data, isLoading, error } = useQuery({
    queryKey: ['analytics', params],
    queryFn: () => api.analytics(params),
  })

  const actions = (
    <FiltersBar
      values={filters}
      options={data?.filter_options}
      onChange={setFilters}
    />
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
                  value={`${ov.success_rate.toFixed(1)}%`}
                  delta={ov.delta.success_rate}
                  series={trendPts.map((p) => p.success_rate)}
                  color="var(--ok)"
                />
              </div>

              {/* Trend chart */}
              {trendPts.length > 0 && <TrendChart points={trendPts} />}

              {/* Topology */}
              <TopologySection topology={data.topology} />

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
