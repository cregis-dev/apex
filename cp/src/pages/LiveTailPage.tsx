import { useEffect, useRef, useState, useCallback, useMemo } from 'react'
import { useQuery } from '@tanstack/react-query'
import Topbar from '../components/Topbar.tsx'
import Icon from '../components/Icon.tsx'
import Drawer from '../components/Drawer.tsx'
import { api } from '../lib/api.ts'
import type { UsageRecord } from '../lib/types.ts'

const MAX_ROWS = 200
const POLL_MS = 2000

const STATUS_COLOR: Record<string, string> = {
  success: 'var(--ok)',
  error: 'var(--err)',
  fallback_error: 'var(--err)',
  fallback_success: 'var(--warn)',
}
function statusColor(s: string) { return STATUS_COLOR[s] ?? 'var(--muted)' }
function fmtMs(ms: number | null) { return ms == null ? '—' : ms >= 1000 ? `${(ms / 1000).toFixed(2)}s` : `${Math.round(ms)}ms` }
function fmt(n: number) {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`
  if (n >= 1_000) return `${(n / 1_000).toFixed(1)}k`
  return String(n)
}
function fmtTs(ts: string) {
  try { const d = new Date(ts.replace(' ', 'T')); return `${String(d.getHours()).padStart(2,'0')}:${String(d.getMinutes()).padStart(2,'0')}:${String(d.getSeconds()).padStart(2,'0')}.${String(d.getMilliseconds()).padStart(3,'0')}` }
  catch { return ts }
}

function RecordDetail({ record, group, onClose }: { record: UsageRecord; group?: string; onClose: () => void }) {
  const curlCmd = `curl -X POST http://localhost/v1/chat/completions \\
  -H "Authorization: Bearer <team-key>" \\
  -H "Content-Type: application/json" \\
  -d '{"model": "${record.model}", "messages": [{"role": "user", "content": "..."}]}'`

  return (
    <Drawer open title="Request Detail" sub={`#${record.id}`} onClose={onClose} width={440}>
      <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 10, marginBottom: 20 }}>
        {[
          { label: 'Status', value: record.status_code ?? record.status, color: statusColor(record.status) },
          { label: 'Latency', value: fmtMs(record.latency_ms), color: record.latency_ms != null && record.latency_ms > 1000 ? 'var(--warn)' : undefined },
          { label: 'Tokens', value: fmt(record.input_tokens + record.output_tokens) },
          { label: 'Route', value: record.router },
        ].map((m) => (
          <div key={m.label} className="card" style={{ padding: '10px 14px' }}>
            <div style={{ fontSize: 11, color: 'var(--muted)', marginBottom: 4 }}>{m.label}</div>
            <div style={{ fontWeight: 600, fontSize: 15, color: m.color }}>{String(m.value)}</div>
          </div>
        ))}
      </div>

      <div style={{ marginBottom: 16 }}>
        <div style={{ fontSize: 12, fontWeight: 500, color: 'var(--ink-2)', marginBottom: 6 }}>Request</div>
        <pre style={{
          background: 'var(--ink)', color: '#e8e0d8',
          borderRadius: 'var(--r-sm)', padding: '10px 12px',
          fontSize: 11, fontFamily: 'var(--font-mono)',
          whiteSpace: 'pre-wrap', wordBreak: 'break-all', margin: 0, overflowX: 'auto',
        }}>
          POST /v1/chat/completions{'\n'}
          model: {record.model}{'\n'}
          team: {record.team_id}{'\n'}
          {group ? <>group: {group}{'\n'}</> : null}
          channel: {record.final_channel || record.channel}
        </pre>
      </div>

      <div style={{ marginBottom: 16 }}>
        <div style={{ fontSize: 12, fontWeight: 500, color: 'var(--ink-2)', marginBottom: 6 }}>Response</div>
        <pre style={{
          background: 'var(--surface-2)', border: '1px solid var(--border)',
          borderRadius: 'var(--r-sm)', padding: '10px 12px',
          fontSize: 11, fontFamily: 'var(--font-mono)',
          whiteSpace: 'pre-wrap', wordBreak: 'break-all', margin: 0,
        }}>
          {record.status_code ?? record.status}{'\n'}
          input_tokens: {record.input_tokens}{'\n'}
          output_tokens: {record.output_tokens}{'\n'}
          latency: {fmtMs(record.latency_ms)}
          {record.error_message ? `\n\nerror: ${record.error_message}` : ''}
        </pre>
      </div>

      <div style={{ display: 'flex', gap: 8, marginBottom: 16 }}>
        <button className="btn btn-sm" onClick={() => navigator.clipboard.writeText(curlCmd)}>
          <Icon name="copy" size={13} /> Copy cURL
        </button>
        <button className="btn btn-sm" style={{ color: 'var(--muted)', cursor: 'not-allowed' }} disabled title="Replay not supported">
          <Icon name="play" size={13} /> Replay
        </button>
      </div>

      <div style={{
        padding: '8px 12px', background: 'var(--info-soft)',
        borderRadius: 'var(--r-sm)', fontSize: 12, color: 'var(--info)',
      }}>
        Replay is not supported in this version.
      </div>
    </Drawer>
  )
}

export default function LiveTailPage() {
  const [rows, setRows] = useState<UsageRecord[]>([])
  const [running, setRunning] = useState(true)
  const [selected, setSelected] = useState<UsageRecord | null>(null)
  const [newCount, setNewCount] = useState(0)
  const [teamFilter, setTeamFilter] = useState('')
  const [modelFilter, setModelFilter] = useState('')
  const [statusFilter, setStatusFilter] = useState('')
  const shownIds = useRef(new Set<number>())
  const latestId = useRef<number | null>(null)
  const initialized = useRef(false)

  const { data: analytics } = useQuery({
    queryKey: ['analytics-filter-options', '1h'],
    queryFn: () => api.analytics({ range: '1h' }),
    staleTime: 60_000,
  })

  // Teams carry an optional group label; records only store team_id, so resolve
  // the group client-side to show it alongside the team. Only teams with a
  // non-empty group are mapped, so rows for ungrouped teams render no group.
  const { data: teamsData } = useQuery({
    queryKey: ['teams-groups'],
    queryFn: () => api.teams(),
    staleTime: 60_000,
  })

  const groupByTeam = useMemo(() => {
    const m = new Map<string, string>()
    for (const t of teamsData?.data ?? []) {
      const g = t.group?.trim()
      if (g) m.set(t.id, g)
    }
    return m
  }, [teamsData])

  const fetchRecords = useCallback(async (initial = false) => {
    try {
      const filterParams: Record<string, string | number> = {}
      if (teamFilter) filterParams.team_id = teamFilter
      if (modelFilter) filterParams.model = modelFilter
      if (statusFilter) filterParams.status = statusFilter

      const params = initial
        ? { range: '1h' as const, limit: 30, ...filterParams }
        : { range: '1h' as const, limit: 50, ...filterParams, ...(latestId.current != null ? { since_id: latestId.current } : {}) }

      const res = await api.records(params)

      const fresh = res.data.filter((r) => !shownIds.current.has(r.id))
      if (!fresh.length) return

      fresh.forEach((r) => shownIds.current.add(r.id))
      if (fresh[0]) latestId.current = fresh[0].id

      if (initial) {
        setRows(fresh)
      } else {
        setNewCount((n) => n + fresh.length)
        setRows((prev) => {
          const combined = [...fresh, ...prev]
          return combined.slice(0, MAX_ROWS)
        })
      }
    } catch {
      // silently ignore poll errors
    }
  }, [teamFilter, modelFilter, statusFilter])

  // Reset rows when filters change
  useEffect(() => {
    shownIds.current = new Set()
    latestId.current = null
    setRows([])
    setNewCount(0)
    void fetchRecords(true)
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [teamFilter, modelFilter, statusFilter])

  // initial load
  useEffect(() => {
    if (initialized.current) return
    initialized.current = true
    void fetchRecords(true)
  }, [fetchRecords])

  // polling interval
  useEffect(() => {
    if (!running) return
    const id = setInterval(() => { void fetchRecords(false) }, POLL_MS)
    return () => clearInterval(id)
  }, [running, fetchRecords])

  const livePill = running
    ? <span style={{ display: 'inline-flex', alignItems: 'center', gap: 5, padding: '2px 8px', borderRadius: 999, background: 'var(--err-soft)', fontSize: 12, fontWeight: 500 }}>
        <span style={{ width: 6, height: 6, borderRadius: '50%', background: 'var(--err)', animation: 'blink-rec 1.4s ease-in-out infinite' }} />
        LIVE
      </span>
    : <span className="badge">PAUSED</span>

  const opts = analytics?.filter_options
  const actions = (
    <div style={{ display: 'flex', gap: 8, alignItems: 'center', flexWrap: 'wrap' }}>
      <select
        className="select btn-sm"
        value={teamFilter}
        onChange={(e) => setTeamFilter(e.target.value)}
        style={{ height: 28, fontSize: 12 }}
        title="Filter by team"
      >
        <option value="">All teams</option>
        {(opts?.teams ?? []).map((t) => <option key={t} value={t}>{t}</option>)}
      </select>
      <select
        className="select btn-sm"
        value={modelFilter}
        onChange={(e) => setModelFilter(e.target.value)}
        style={{ height: 28, fontSize: 12 }}
        title="Filter by model"
      >
        <option value="">All models</option>
        {(opts?.models ?? []).map((m) => <option key={m} value={m}>{m}</option>)}
      </select>
      <select
        className="select btn-sm"
        value={statusFilter}
        onChange={(e) => setStatusFilter(e.target.value)}
        style={{ height: 28, fontSize: 12 }}
        title="Filter by status"
      >
        <option value="">All statuses</option>
        <option value="success">Success</option>
        <option value="error">Error</option>
        <option value="fallback_success">Fallback success</option>
        <option value="fallback_error">Fallback error</option>
      </select>
      {newCount > 0 && !running && (
        <span style={{ fontSize: 12, color: 'var(--brand)' }}>{newCount} new</span>
      )}
      <button
        className="btn btn-sm"
        onClick={() => { setRunning((r) => !r); setNewCount(0) }}
      >
        <Icon name={running ? 'pause' : 'play'} size={13} />
        {running ? 'Pause' : 'Resume'}
      </button>
    </div>
  )

  return (
    <>
      <Topbar
        breadcrumbs={[{ label: 'Operate' }, { label: 'Live Tail' }]}
        actions={actions}
      />
      <div className="page-pad">
        <div className="page-head" style={{ display: 'flex', alignItems: 'flex-start', justifyContent: 'space-between' }}>
          <div>
            <h1 className="page-title" style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
              Live Tail {livePill}
            </h1>
            <p className="page-sub">Streaming requests in real time. Click any row to inspect.</p>
          </div>
        </div>

        <div className="card" style={{ maxHeight: 'calc(100vh - 220px)', overflowY: 'auto' }}>
          {rows.length === 0 ? (
            <div style={{ display: 'flex', flexDirection: 'column', alignItems: 'center', justifyContent: 'center', padding: 64, color: 'var(--muted)' }}>
              <span className="spinner" style={{ width: 20, height: 20, marginBottom: 16 }} />
              <div style={{ fontWeight: 500, color: 'var(--ink-2)' }}>Waiting for requests…</div>
              <div style={{ fontSize: 13, marginTop: 4 }}>Requests will appear here as they arrive.</div>
            </div>
          ) : (
            <table className="table" style={{ tableLayout: 'fixed' }}>
              <thead style={{ position: 'sticky', top: 0, zIndex: 1 }}>
                <tr>
                  <th style={{ width: 110 }}>Time</th>
                  <th style={{ width: 80 }}>Status</th>
                  <th>Model</th>
                  <th>Channel</th>
                  <th>Team</th>
                  <th style={{ width: 80, textAlign: 'right' }}>Latency</th>
                  <th style={{ width: 70, textAlign: 'right' }}>Tokens</th>
                </tr>
              </thead>
              <tbody>
                {rows.map((r, i) => (
                  <tr
                    key={r.id}
                    className="row-hover"
                    onClick={() => setSelected(r)}
                    style={{
                      cursor: 'pointer',
                      background: selected?.id === r.id ? 'var(--brand-soft)' : undefined,
                      animation: i < 5 && !initialized.current ? 'slide-in-row 220ms ease' : undefined,
                    }}
                  >
                    <td style={{ fontFamily: 'var(--font-mono)', fontSize: 11, color: 'var(--muted)' }}>
                      {fmtTs(r.timestamp)}
                    </td>
                    <td>
                      <span style={{ fontFamily: 'var(--font-mono)', fontSize: 12, color: statusColor(r.status) }}>
                        {r.status_code ?? r.status}
                      </span>
                    </td>
                    <td style={{ fontFamily: 'var(--font-mono)', fontSize: 12, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
                      {r.model}
                    </td>
                    <td style={{ fontSize: 12, color: 'var(--muted)', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
                      {r.channel}
                    </td>
                    <td style={{ fontSize: 13, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
                      {r.team_id}
                      {groupByTeam.get(r.team_id) && (
                        <div style={{ fontSize: 11, color: 'var(--muted)', overflow: 'hidden', textOverflow: 'ellipsis' }}>
                          {groupByTeam.get(r.team_id)}
                        </div>
                      )}
                    </td>
                    <td style={{ textAlign: 'right', fontFamily: 'var(--font-mono)', fontSize: 12, color: r.latency_ms != null && r.latency_ms > 1000 ? 'var(--warn)' : undefined }}>
                      {fmtMs(r.latency_ms)}
                    </td>
                    <td style={{ textAlign: 'right', fontFamily: 'var(--font-mono)', fontSize: 12 }}>
                      {fmt(r.input_tokens + r.output_tokens)}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}
        </div>
      </div>

      {selected && <RecordDetail record={selected} group={groupByTeam.get(selected.team_id)} onClose={() => setSelected(null)} />}
    </>
  )
}
