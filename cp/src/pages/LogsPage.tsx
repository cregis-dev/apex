import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import Topbar from '../components/Topbar.tsx'
import Icon from '../components/Icon.tsx'
import { streamLogs } from '../lib/api.ts'
import type { LogEntry } from '../lib/types.ts'

/** Cap retained lines so a long-running tail can't grow without bound. */
const MAX_LINES = 5_000
/** Backlog replayed on connect. */
const BACKLOG = 500
/** Reconnect delay after the stream drops. */
const RETRY_MS = 3_000

const LEVELS = ['TRACE', 'DEBUG', 'INFO', 'WARN', 'ERROR'] as const

/** Mirrors the terminal colors `apex logs` uses (src/logs.rs). */
const LEVEL_COLOR: Record<string, string> = {
  TRACE: 'var(--muted-2)',
  DEBUG: 'var(--info)',
  INFO: 'var(--ok)',
  WARN: 'var(--warn)',
  ERROR: 'var(--err)',
}

function fmtTime(ts: string) {
  // Server sends "YYYY-MM-DD HH:MM:SS.mmm"; the clock time is what matters here.
  const spaceAt = ts.indexOf(' ')
  return spaceAt === -1 ? ts : ts.slice(spaceAt + 1)
}

export default function LogsPage() {
  const [lines, setLines] = useState<LogEntry[]>([])
  const [level, setLevel] = useState('')
  const [query, setQuery] = useState('')
  const [running, setRunning] = useState(true)
  const [connected, setConnected] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [follow, setFollow] = useState(true)

  const scrollRef = useRef<HTMLDivElement>(null)
  // Survives reconnects so we resume instead of replaying the whole backlog.
  const lastSeq = useRef<number | null>(null)
  const paused = useRef<LogEntry[]>([])

  // Pausing must not drop lines — buffer them and flush on resume.
  const runningRef = useRef(running)
  useEffect(() => {
    runningRef.current = running
    if (running && paused.current.length) {
      const buffered = paused.current
      paused.current = []
      setLines((prev) => [...prev, ...buffered].slice(-MAX_LINES))
    }
  }, [running])

  useEffect(() => {
    let cancelled = false
    let retry: ReturnType<typeof setTimeout> | undefined
    let unsubscribe: (() => void) | undefined

    const connect = () => {
      if (cancelled) return
      unsubscribe = streamLogs(
        {
          level: level || undefined,
          limit: BACKLOG,
          ...(lastSeq.current != null ? { after_seq: lastSeq.current } : {}),
        },
        {
          onOpen: () => {
            setConnected(true)
            setError(null)
          },
          onEntry: (entry) => {
            lastSeq.current = entry.seq
            if (runningRef.current) {
              setLines((prev) => [...prev, entry].slice(-MAX_LINES))
            } else {
              paused.current = [...paused.current, entry].slice(-MAX_LINES)
            }
          },
          onError: (err) => {
            setConnected(false)
            setError(err instanceof Error ? err.message : String(err))
            retry = setTimeout(connect, RETRY_MS)
          },
        },
      )
    }

    connect()
    return () => {
      cancelled = true
      if (retry) clearTimeout(retry)
      unsubscribe?.()
      setConnected(false)
    }
    // Changing the level filter reopens the stream server-side.
  }, [level])

  // Reset the view (and the resume cursor) when the level filter changes, so
  // the new stream starts from a fresh backlog at that level.
  useEffect(() => {
    lastSeq.current = null
    paused.current = []
    setLines([])
  }, [level])

  const visible = useMemo(() => {
    const needle = query.trim().toLowerCase()
    if (!needle) return lines
    return lines.filter(
      (l) =>
        l.message.toLowerCase().includes(needle) ||
        l.target.toLowerCase().includes(needle) ||
        (l.request_id?.toLowerCase().includes(needle) ?? false),
    )
  }, [lines, query])

  // Keep the newest line in view unless the user scrolled up to read history.
  useEffect(() => {
    if (!follow) return
    const el = scrollRef.current
    if (el) el.scrollTop = el.scrollHeight
  }, [visible, follow])

  const onScroll = useCallback(() => {
    const el = scrollRef.current
    if (!el) return
    const atBottom = el.scrollHeight - el.scrollTop - el.clientHeight < 40
    setFollow(atBottom)
  }, [])

  const copyAll = useCallback(() => {
    const text = visible
      .map((l) => `${l.timestamp} ${l.level} ${l.target}: ${l.message}`)
      .join('\n')
    void navigator.clipboard.writeText(text)
  }, [visible])

  const statusPill = connected ? (
    <span style={{ display: 'inline-flex', alignItems: 'center', gap: 5, padding: '2px 8px', borderRadius: 999, background: 'var(--err-soft)', fontSize: 12, fontWeight: 500 }}>
      <span style={{ width: 6, height: 6, borderRadius: '50%', background: 'var(--err)', animation: 'blink-rec 1.4s ease-in-out infinite' }} />
      LIVE
    </span>
  ) : (
    <span className="badge" style={{ color: 'var(--warn)' }}>RECONNECTING…</span>
  )

  const actions = (
    <div style={{ display: 'flex', gap: 8, alignItems: 'center', flexWrap: 'wrap' }}>
      <input
        className="input btn-sm"
        placeholder="Filter text…"
        value={query}
        onChange={(e) => setQuery(e.target.value)}
        style={{ height: 28, fontSize: 12, width: 180 }}
      />
      <select
        className="select btn-sm"
        value={level}
        onChange={(e) => setLevel(e.target.value)}
        style={{ height: 28, fontSize: 12 }}
        title="Minimum severity"
      >
        <option value="">All levels</option>
        {LEVELS.map((l) => <option key={l} value={l}>{l} and above</option>)}
      </select>
      <button className="btn btn-sm" onClick={() => { setLines([]); paused.current = [] }} title="Clear the view">
        <Icon name="trash" size={13} /> Clear
      </button>
      <button className="btn btn-sm" onClick={copyAll} title="Copy visible lines">
        <Icon name="copy" size={13} /> Copy
      </button>
      <button className="btn btn-sm" onClick={() => setRunning((r) => !r)}>
        <Icon name={running ? 'pause' : 'play'} size={13} />
        {running ? 'Pause' : `Resume${paused.current.length ? ` (${paused.current.length})` : ''}`}
      </button>
    </div>
  )

  return (
    <>
      <Topbar breadcrumbs={[{ label: 'Operate' }, { label: 'Logs' }]} actions={actions} />
      <div className="page-pad">
        <div className="page-head">
          <h1 className="page-title" style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
            Logs {statusPill}
          </h1>
          <p className="page-sub">
            Live gateway logs, the same stream as <code>apex logs</code>. Verbosity follows{' '}
            <code>logging.level</code> — raise it in Settings to see DEBUG lines.
          </p>
        </div>

        {error && (
          <div className="card" style={{ padding: '10px 14px', marginBottom: 12, color: 'var(--err)', fontSize: 13 }}>
            Stream disconnected: {error}. Retrying every {RETRY_MS / 1000}s…
          </div>
        )}

        <div
          ref={scrollRef}
          onScroll={onScroll}
          className="card"
          style={{
            background: 'var(--ink)',
            padding: '10px 14px',
            height: 'calc(100vh - 260px)',
            overflow: 'auto',
            fontFamily: 'var(--font-mono)',
            fontSize: 12,
            lineHeight: 1.6,
          }}
        >
          {visible.length === 0 ? (
            <div style={{ color: 'var(--muted)', padding: 24, textAlign: 'center' }}>
              {lines.length === 0 ? 'Waiting for log lines…' : 'No lines match the filter.'}
            </div>
          ) : (
            visible.map((l) => (
              <div key={l.seq} style={{ display: 'flex', gap: 8, whiteSpace: 'pre-wrap', wordBreak: 'break-word' }}>
                <span style={{ color: 'oklch(0.58 0.01 60)', flexShrink: 0 }}>{fmtTime(l.timestamp)}</span>
                <span style={{ color: LEVEL_COLOR[l.level] ?? 'var(--muted)', flexShrink: 0, width: 44 }}>
                  {l.level}
                </span>
                {l.request_id && (
                  <span style={{ color: 'var(--warn)', flexShrink: 0 }} title={`request_id ${l.request_id}`}>
                    {l.request_id.slice(0, 8)}
                  </span>
                )}
                <span style={{ color: 'oklch(0.72 0.09 220)', flexShrink: 0 }}>{l.target}</span>
                <span style={{ color: '#e8e0d8' }}>{l.message}</span>
              </div>
            ))
          )}
        </div>

        <div style={{ display: 'flex', justifyContent: 'space-between', marginTop: 8, fontSize: 12, color: 'var(--muted)' }}>
          <span>
            {visible.length.toLocaleString()} line{visible.length === 1 ? '' : 's'}
            {query && lines.length !== visible.length ? ` (of ${lines.length.toLocaleString()})` : ''}
            {lines.length >= MAX_LINES ? ` · oldest trimmed at ${MAX_LINES.toLocaleString()}` : ''}
          </span>
          {!follow && (
            <button
              className="btn btn-sm"
              onClick={() => {
                setFollow(true)
                const el = scrollRef.current
                if (el) el.scrollTop = el.scrollHeight
              }}
            >
              <Icon name="arrow-right" size={13} /> Jump to latest
            </button>
          )}
        </div>
      </div>
    </>
  )
}
