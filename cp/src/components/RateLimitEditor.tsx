import { useState } from 'react'
import { useQuery } from '@tanstack/react-query'
import Modal from './Modal.tsx'
import Icon from './Icon.tsx'
import { api } from '../lib/api.ts'
import type { AdminTeam, RateLimit } from '../lib/types.ts'

function fmt(n: number | null): string {
  if (n == null) return '—'
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`
  if (n >= 1_000) return `${(n / 1_000).toFixed(1)}k`
  return String(n)
}

function parseInt32(value: string): number | null {
  const trimmed = value.trim()
  if (!trimmed) return null
  const n = Number(trimmed)
  if (!Number.isFinite(n) || n < 0) return null
  return Math.floor(n)
}

/** Round up to a clean 1 / 2 / 5 × 10ⁿ figure for limit suggestions. */
function niceCeil(n: number): number {
  if (n <= 0) return 0
  const pow = 10 ** Math.floor(Math.log10(n))
  const f = n / pow
  const nice = f <= 1 ? 1 : f <= 2 ? 2 : f <= 5 ? 5 : 10
  return nice * pow
}

function perMin(total: number): string {
  const v = total / 60
  if (v >= 10) return String(Math.round(v))
  if (v >= 1) return v.toFixed(1)
  return v.toFixed(2)
}

function SuggestLink({ value, onUse }: { value: number; onUse: () => void }) {
  return (
    <button
      type="button"
      onClick={onUse}
      title="Fill with the suggested value"
      style={{
        marginTop: 6, padding: 0, fontSize: 11, color: 'var(--brand-ink)',
        display: 'inline-flex', alignItems: 'center', gap: 4, background: 'none', border: 'none', cursor: 'pointer',
      }}
    >
      <Icon name="zap" size={11} /> Use {value.toLocaleString()}
    </button>
  )
}

interface RateLimitEditorProps {
  team: AdminTeam
  busy: boolean
  error?: string
  onCancel: () => void
  onSubmit: (rl: RateLimit | null) => void
}

/**
 * Focused editor for a team's RPM/TPM limit. Fetches the team's last-24h traffic
 * so the operator has a real reference for what to set, with one-click suggestions.
 * Mount it conditionally (only when a team is selected) so its state resets per team.
 */
export default function RateLimitEditor({ team, busy, error, onCancel, onSubmit }: RateLimitEditorProps) {
  const initial = team.policy.rate_limit
  const hadLimit = initial?.rpm != null || initial?.tpm != null
  const [rpm, setRpm] = useState(initial?.rpm != null ? String(initial.rpm) : '')
  const [tpm, setTpm] = useState(initial?.tpm != null ? String(initial.tpm) : '')

  // Reference: this team's recent traffic, to ground the suggested limits.
  const { data: ref, isLoading: refLoading } = useQuery({
    queryKey: ['analytics', '24h', team.id],
    queryFn: () => api.analytics({ range: '24h', team_id: team.id }),
    staleTime: 60_000,
  })
  const points = ref?.trend.points ?? []
  const totalReq = ref?.overview.total_requests ?? 0
  const totalTok = ref?.overview.total_tokens ?? 0
  const peakReq = points.reduce((m, p) => Math.max(m, p.requests), 0)
  const peakTok = points.reduce((m, p) => Math.max(m, p.total_tokens), 0)
  const hasData = totalReq > 0
  // Busiest-hour per-minute average × 3 burst headroom, with a sane floor.
  const suggestRpm = peakReq > 0 ? niceCeil(Math.max((peakReq / 60) * 3, 10)) : null
  const suggestTpm = peakTok > 0 ? niceCeil(Math.max((peakTok / 60) * 3, 1000)) : null

  const rpmN = parseInt32(rpm)
  const tpmN = parseInt32(tpm)
  const rpmInvalid = rpm.trim() !== '' && rpmN == null
  const tpmInvalid = tpm.trim() !== '' && tpmN == null
  const hasLimit = rpmN != null || tpmN != null
  const canSave = !rpmInvalid && !tpmInvalid && (hasLimit || hadLimit)

  return (
    <Modal
      open
      onClose={busy ? () => {} : onCancel}
      title={`${hadLimit ? 'Edit' : 'Set'} rate limit · ${team.id}`}
      width={480}
      footer={
        <>
          {hadLimit && (
            <button
              className="btn btn-sm"
              style={{ marginRight: 'auto', color: 'var(--err)' }}
              disabled={busy}
              onClick={() => onSubmit(null)}
            >
              Remove limit
            </button>
          )}
          <button className="btn btn-sm" onClick={onCancel} disabled={busy}>Cancel</button>
          <button
            className="btn btn-primary btn-sm"
            disabled={busy || !canSave}
            onClick={() => onSubmit(hasLimit ? { rpm: rpmN, tpm: tpmN } : null)}
          >
            {busy ? <span className="spinner" style={{ width: 12, height: 12 }} /> : null}
            Save
          </button>
        </>
      }
    >
      {error && (
        <div style={{ padding: '8px 12px', marginBottom: 14, borderRadius: 'var(--r-sm)', background: 'var(--err-soft)', color: 'var(--err)', fontSize: 13 }}>
          {error}
        </div>
      )}

      {/* Reference: grounds the numbers in this team's real load */}
      <div style={{
        marginBottom: 16, padding: '10px 12px',
        background: 'var(--surface-2)', border: '1px solid var(--border)',
        borderRadius: 'var(--r-md)', fontSize: 12,
      }}>
        <div style={{
          display: 'flex', alignItems: 'center', gap: 6,
          color: 'var(--muted)', fontWeight: 500,
          marginBottom: refLoading || hasData ? 8 : 0,
        }}>
          <Icon name="activity" size={13} /> Recent traffic · last 24h
        </div>
        {refLoading ? (
          <div style={{ display: 'flex', alignItems: 'center', gap: 8, color: 'var(--muted)' }}>
            <span className="spinner" style={{ width: 12, height: 12 }} /> Reading usage…
          </div>
        ) : !hasData ? (
          <div style={{ color: 'var(--muted)' }}>
            No requests recorded in the last 24h — set a limit based on the load you expect.
          </div>
        ) : (
          <div style={{ display: 'grid', gap: 5 }}>
            <div style={{ display: 'flex', justifyContent: 'space-between' }}>
              <span style={{ color: 'var(--muted)' }}>Total</span>
              <span className="mono" style={{ color: 'var(--ink-2)' }}>{totalReq.toLocaleString()} req · {fmt(totalTok)} tok</span>
            </div>
            <div style={{ display: 'flex', justifyContent: 'space-between' }}>
              <span style={{ color: 'var(--muted)' }}>Busiest hour</span>
              <span className="mono" style={{ color: 'var(--ink-2)' }}>
                {peakReq.toLocaleString()} req (~{perMin(peakReq)}/min){peakTok > 0 ? ` · ${fmt(peakTok)} tok` : ''}
              </span>
            </div>
          </div>
        )}
      </div>

      {/* Inputs */}
      <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 12 }}>
        <div>
          <div style={{ fontSize: 12, fontWeight: 500, color: 'var(--ink-2)', marginBottom: 6 }}>Requests / minute (RPM)</div>
          <input
            className="input"
            value={rpm}
            onChange={(e) => setRpm(e.target.value)}
            placeholder="unlimited"
            inputMode="numeric"
            autoFocus
            style={{ width: '100%', borderColor: rpmInvalid ? 'var(--err)' : undefined }}
          />
          {suggestRpm != null && <SuggestLink value={suggestRpm} onUse={() => setRpm(String(suggestRpm))} />}
        </div>
        <div>
          <div style={{ fontSize: 12, fontWeight: 500, color: 'var(--ink-2)', marginBottom: 6 }}>Tokens / minute (TPM)</div>
          <input
            className="input"
            value={tpm}
            onChange={(e) => setTpm(e.target.value)}
            placeholder="unlimited"
            inputMode="numeric"
            style={{ width: '100%', borderColor: tpmInvalid ? 'var(--err)' : undefined }}
          />
          {suggestTpm != null && <SuggestLink value={suggestTpm} onUse={() => setTpm(String(suggestTpm))} />}
        </div>
      </div>

      <div style={{ marginTop: 12, fontSize: 11, color: 'var(--muted)' }}>
        {hasData && (suggestRpm != null || suggestTpm != null)
          ? 'Suggestions add ~3× headroom over the busiest hour’s per-minute average. Leave a field blank to keep that dimension unlimited.'
          : 'Leave a field blank to keep that dimension unlimited.'}
      </div>
    </Modal>
  )
}
