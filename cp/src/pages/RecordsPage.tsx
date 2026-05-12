import { useState, useCallback } from 'react'
import { useQuery } from '@tanstack/react-query'
import Topbar from '../components/Topbar.tsx'
import Icon from '../components/Icon.tsx'
import Drawer from '../components/Drawer.tsx'
import Empty from '../components/Empty.tsx'
import { api } from '../lib/api.ts'
import type { TimeRange, UsageRecord, RecordsParams } from '../lib/types.ts'

const RANGES: { label: string; value: TimeRange }[] = [
  { label: 'Last 1h', value: '1h' },
  { label: 'Last 24h', value: '24h' },
  { label: 'Last 7d', value: '7d' },
  { label: 'Last 30d', value: '30d' },
]

const PAGE_SIZE = 50

const STATUS_COLOR: Record<string, string> = {
  success: 'var(--ok)',
  error: 'var(--err)',
  fallback_error: 'var(--err)',
  fallback_success: 'var(--warn)',
}

function statusColor(s: string): string {
  return STATUS_COLOR[s] ?? 'var(--muted)'
}

function fmtMs(ms: number | null): string {
  if (ms == null) return '—'
  return ms >= 1000 ? `${(ms / 1000).toFixed(2)}s` : `${Math.round(ms)}ms`
}

function fmt(n: number): string {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`
  if (n >= 1_000) return `${(n / 1_000).toFixed(1)}k`
  return String(n)
}

function fmtTs(ts: string): string {
  try {
    return new Date(ts.replace(' ', 'T')).toLocaleString()
  } catch {
    return ts
  }
}

function RecordInspector({ record, onClose }: { record: UsageRecord; onClose: () => void }) {
  return (
    <Drawer open title="Request Detail" sub={record.id.toString()} onClose={onClose} width={480}>
      {/* Meta grid */}
      <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 10, marginBottom: 20 }}>
        {[
          { label: 'Status', value: record.status, color: statusColor(record.status) },
          { label: 'Latency', value: fmtMs(record.latency_ms), color: record.latency_ms != null && record.latency_ms > 1000 ? 'var(--warn)' : undefined },
          { label: 'Tokens', value: fmt(record.input_tokens + record.output_tokens) },
          { label: 'Channel', value: record.final_channel || record.channel },
        ].map((m) => (
          <div key={m.label} className="card" style={{ padding: '10px 14px' }}>
            <div style={{ fontSize: 11, color: 'var(--muted)', marginBottom: 4 }}>{m.label}</div>
            <div style={{ fontWeight: 600, fontSize: 15, color: m.color }}>{m.value}</div>
          </div>
        ))}
      </div>

      {/* Fields */}
      <table style={{ width: '100%', fontSize: 13, borderCollapse: 'collapse' }}>
        <tbody>
          {[
            ['Time', fmtTs(record.timestamp)],
            ['Team', record.team_id],
            ['Router', record.router],
            ['Model', record.model],
            ['Input tokens', record.input_tokens.toLocaleString()],
            ['Output tokens', record.output_tokens.toLocaleString()],
            ['Request ID', record.request_id ?? '—'],
            ['Status code', record.status_code?.toString() ?? '—'],
            ['Fallback', record.fallback_triggered ? 'Yes' : 'No'],
          ].map(([k, v]) => (
            <tr key={k as string} style={{ borderBottom: '1px solid var(--divider)' }}>
              <td style={{ padding: '8px 0', color: 'var(--muted)', width: '40%' }}>{k}</td>
              <td style={{ padding: '8px 0', fontFamily: 'var(--font-mono)', fontSize: 12 }}>{v}</td>
            </tr>
          ))}
        </tbody>
      </table>

      {record.error_message && (
        <div style={{ marginTop: 16 }}>
          <div style={{ fontSize: 12, fontWeight: 500, color: 'var(--err)', marginBottom: 6 }}>Error</div>
          <pre style={{
            background: 'var(--surface-2)', border: '1px solid var(--border)',
            borderRadius: 'var(--r-sm)', padding: '10px 12px',
            fontSize: 12, fontFamily: 'var(--font-mono)',
            whiteSpace: 'pre-wrap', wordBreak: 'break-all', margin: 0,
          }}>
            {record.error_message}
          </pre>
        </div>
      )}

      {/* Replay note */}
      <div style={{
        marginTop: 20, padding: '10px 14px',
        background: 'var(--info-soft)', borderRadius: 'var(--r-sm)',
        fontSize: 12, color: 'var(--info)',
      }}>
        Replay is not supported in this version.
      </div>
    </Drawer>
  )
}

export default function RecordsPage() {
  const [range, setRange] = useState<TimeRange>('24h')
  const [offset, setOffset] = useState(0)
  const [selected, setSelected] = useState<UsageRecord | null>(null)

  const params: RecordsParams = { range, limit: PAGE_SIZE, offset }

  const { data, isLoading, error } = useQuery({
    queryKey: ['records', range, offset],
    queryFn: () => api.records(params),
  })

  const records = data?.data ?? []
  const total = data?.total ?? 0
  const pages = Math.ceil(total / PAGE_SIZE)
  const page = Math.floor(offset / PAGE_SIZE)

  const handleExport = useCallback(() => {
    if (!records.length) return
    const header = 'id,timestamp,team_id,router,channel,model,status,latency_ms,input_tokens,output_tokens'
    const rows = records.map((r) =>
      [r.id, r.timestamp, r.team_id, r.router, r.channel, r.model, r.status, r.latency_ms ?? '', r.input_tokens, r.output_tokens].join(',')
    )
    const blob = new Blob([[header, ...rows].join('\n')], { type: 'text/csv' })
    const a = document.createElement('a')
    a.href = URL.createObjectURL(blob)
    a.download = `apex-records-${range}.csv`
    a.click()
  }, [records, range])

  const actions = (
    <div style={{ display: 'flex', gap: 8 }}>
      <select
        className="select btn-sm"
        value={range}
        onChange={(e) => { setRange(e.target.value as TimeRange); setOffset(0) }}
        style={{ height: 28, fontSize: 12 }}
      >
        {RANGES.map((r) => (
          <option key={r.value} value={r.value}>{r.label}</option>
        ))}
      </select>
      <button className="btn btn-sm" onClick={handleExport} disabled={!records.length}>
        <Icon name="download" size={13} /> CSV
      </button>
    </div>
  )

  return (
    <>
      <Topbar breadcrumbs={[{ label: 'Operate' }, { label: 'Records' }]} actions={actions} />
      <div className="page-pad">
        <div className="page-head">
          <h1 className="page-title">Records</h1>
          <p className="page-sub">Searchable history of every request through the gateway.</p>
        </div>

        {isLoading && (
          <div style={{ display: 'flex', justifyContent: 'center', padding: 48 }}>
            <span className="spinner" style={{ width: 20, height: 20 }} />
          </div>
        )}

        {error && (
          <div style={{ padding: '12px 16px', background: 'var(--err-soft)', color: 'var(--err)', borderRadius: 'var(--r-md)', fontSize: 13 }}>
            Failed to load records.
          </div>
        )}

        {!isLoading && !error && (
          <div className="card">
            {records.length === 0 ? (
              <Empty icon="list" title="No records in this period" sub="Requests will be logged here as they flow through the gateway." />
            ) : (
              <>
                <div style={{ padding: '10px 16px', borderBottom: '1px solid var(--border)', fontSize: 12, color: 'var(--muted)' }}>
                  {total.toLocaleString()} records · page {page + 1} of {Math.max(1, pages)}
                </div>
                <table className="table">
                  <thead>
                    <tr>
                      <th>Time</th>
                      <th>Team</th>
                      <th>Model</th>
                      <th>Channel</th>
                      <th>Status</th>
                      <th style={{ textAlign: 'right' }}>Latency</th>
                      <th style={{ textAlign: 'right' }}>Tokens</th>
                    </tr>
                  </thead>
                  <tbody>
                    {records.map((r) => (
                      <tr
                        key={r.id}
                        className="row-hover"
                        onClick={() => setSelected(r)}
                        style={{ cursor: 'pointer', background: selected?.id === r.id ? 'var(--brand-soft)' : undefined }}
                      >
                        <td style={{ fontFamily: 'var(--font-mono)', fontSize: 11, color: 'var(--muted)', whiteSpace: 'nowrap' }}>
                          {fmtTs(r.timestamp)}
                        </td>
                        <td style={{ fontSize: 13 }}>{r.team_id}</td>
                        <td style={{ fontFamily: 'var(--font-mono)', fontSize: 12 }}>{r.model}</td>
                        <td style={{ fontSize: 12, color: 'var(--muted)' }}>{r.channel}</td>
                        <td>
                          <span style={{ fontFamily: 'var(--font-mono)', fontSize: 12, color: statusColor(r.status) }}>
                            {r.status_code ?? r.status}
                          </span>
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

                {/* Pagination */}
                {pages > 1 && (
                  <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'flex-end', gap: 8, padding: '12px 16px', borderTop: '1px solid var(--border)' }}>
                    <button className="btn btn-sm" disabled={offset === 0} onClick={() => setOffset(Math.max(0, offset - PAGE_SIZE))}>
                      <Icon name="chevron-left" size={13} /> Prev
                    </button>
                    <span style={{ fontSize: 12, color: 'var(--muted)' }}>{page + 1} / {pages}</span>
                    <button className="btn btn-sm" disabled={offset + PAGE_SIZE >= total} onClick={() => setOffset(offset + PAGE_SIZE)}>
                      Next <Icon name="chevron-right" size={13} />
                    </button>
                  </div>
                )}
              </>
            )}
          </div>
        )}
      </div>

      {selected && <RecordInspector record={selected} onClose={() => setSelected(null)} />}
    </>
  )
}
