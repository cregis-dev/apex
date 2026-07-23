import { useState } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import Topbar from '../components/Topbar.tsx'
import Empty from '../components/Empty.tsx'
import Icon from '../components/Icon.tsx'
import StatusPill from '../components/StatusPill.tsx'
import Modal from '../components/Modal.tsx'
import RateLimitEditor from '../components/RateLimitEditor.tsx'
import FiltersBar, { DEFAULT_FILTERS, filterValuesToParams, type FilterValues } from '../components/FiltersBar.tsx'
import { useToast } from '../components/Toast.tsx'
import { api } from '../lib/api.ts'
import type { AdminTeam, BehaviorMember, BehaviorFlag, RateLimit } from '../lib/types.ts'

// Wire signal keys → display labels (keys must match the backend exactly).
const SIGNAL_LABEL: Record<string, string> = {
  repeat_rate: 'Repeat rate',
  rate_spike: 'Rate spike',
  error_storm: 'Error storm',
  output_zero: 'Zero output',
  spend_spike: 'Spend spike',
  off_hours: 'Off-hours',
}

const ACTION_LABEL: Record<string, string> = {
  observe: 'Observe',
  rate_limit: 'Rate limit',
  disable: 'Disable',
}

function pct(n: number): string {
  return `${(n * 100).toFixed(0)}%`
}

function severityKind(severity: string): 'err' | 'warn' {
  return severity === 'critical' ? 'err' : 'warn'
}

function Metric({ label, value, tone }: { label: string; value: string; tone?: 'muted' | 'warn' }) {
  const color = tone === 'warn' ? 'var(--warn)' : tone === 'muted' ? 'var(--muted)' : 'var(--ink)'
  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 2, minWidth: 62 }}>
      <span style={{ fontSize: 10, textTransform: 'uppercase', letterSpacing: '0.05em', color: 'var(--muted)' }}>{label}</span>
      <span className="mono" style={{ fontSize: 13, color }}>{value}</span>
    </div>
  )
}

function Stat({ label, value, accent }: { label: string; value: string; accent?: string }) {
  return (
    <div className="card" style={{ padding: '16px 18px' }}>
      <div style={{ fontSize: 12, color: 'var(--muted)', marginBottom: 8 }}>{label}</div>
      <div style={{ fontSize: 24, fontWeight: 600, letterSpacing: '-0.02em', color: accent ?? 'var(--ink)' }}>{value}</div>
    </div>
  )
}

/** One flag row: signal label + severity pill + human evidence. */
function FlagRow({ flag }: { flag: BehaviorFlag }) {
  return (
    <div style={{ display: 'flex', alignItems: 'center', gap: 10, padding: '8px 0', borderTop: '1px solid var(--divider)' }}>
      <span style={{ width: 96, fontSize: 13, fontWeight: 500 }}>{SIGNAL_LABEL[flag.signal] ?? flag.signal}</span>
      <StatusPill status={severityKind(flag.severity)} label={flag.severity} />
      <span style={{ flex: 1, fontSize: 12.5, color: 'var(--ink-2)' }}>{flag.detail}</span>
    </div>
  )
}

/**
 * One flagged member: profile metrics, the signals that tripped, and the
 * advisory disposition. Actions are never auto-applied — the operator confirms.
 */
function MemberCard({
  member,
  team,
  onRateLimit,
  onDisable,
}: {
  member: BehaviorMember
  team: AdminTeam | undefined
  onRateLimit: (team: AdminTeam) => void
  onDisable: (team: AdminTeam) => void
}) {
  const p = member.profile
  const suggested = member.suggested_action
  const disabled = team?.enabled === false
  // Tint a metric only when the backend actually flagged its signal, so the
  // evidence bar always agrees with the flag list below — independent of how
  // the operator configured profiling.thresholds.
  const flagged = new Set(member.flags.map((f) => f.signal))

  return (
    <div className="card" style={{ padding: '16px 18px', marginBottom: 12 }}>
      {/* Header */}
      <div style={{ display: 'flex', alignItems: 'center', gap: 10, marginBottom: 14, flexWrap: 'wrap' }}>
        <span
          title={member.severity}
          style={{ width: 8, height: 8, borderRadius: '50%', background: member.severity === 'critical' ? 'var(--err)' : 'var(--warn)', flexShrink: 0 }}
        />
        <span style={{ fontSize: 15, fontWeight: 600 }}>{member.id}</span>
        {member.group && (
          <span style={{ fontSize: 11, color: 'var(--muted)', background: 'var(--surface-2)', padding: '2px 8px', borderRadius: 'var(--r-sm)' }}>
            {member.group}
          </span>
        )}
        <StatusPill status={severityKind(member.severity)} label={`${member.flags.length} flag${member.flags.length > 1 ? 's' : ''}`} />
        <div style={{ flex: 1 }} />
        {suggested !== 'observe' && (
          <span style={{ fontSize: 11.5, color: 'var(--muted)' }}>
            Suggested: <span style={{ color: 'var(--ink-2)', fontWeight: 500 }}>{ACTION_LABEL[suggested] ?? suggested}</span>
          </span>
        )}
      </div>

      {/* Profile metrics */}
      <div style={{ display: 'flex', gap: 20, flexWrap: 'wrap', marginBottom: 14 }}>
        <Metric label="Requests" value={p.requests.toLocaleString()} />
        <Metric label="Repeat" value={pct(p.repeat_rate)} tone={flagged.has('repeat_rate') ? 'warn' : undefined} />
        <Metric label="Errors" value={pct(p.error_ratio)} tone={flagged.has('error_storm') ? 'warn' : undefined} />
        <Metric label="Zero-out" value={pct(p.output_zero_ratio)} tone={flagged.has('output_zero') ? 'warn' : undefined} />
        <Metric label="Night" value={pct(p.night_ratio)} tone={flagged.has('off_hours') ? 'warn' : undefined} />
        {p.rate_z != null && <Metric label="Rate z" value={`${p.rate_z.toFixed(1)}σ`} tone={flagged.has('rate_spike') ? 'warn' : 'muted'} />}
        {p.spend_z != null && <Metric label="Spend z" value={`${p.spend_z.toFixed(1)}σ`} tone={flagged.has('spend_spike') ? 'warn' : 'muted'} />}
      </div>

      {/* Flags */}
      <div style={{ marginBottom: 14 }}>
        {member.flags.map((f) => (
          <FlagRow key={f.signal} flag={f} />
        ))}
      </div>

      {/* Disposition */}
      <div style={{ display: 'flex', alignItems: 'center', gap: 8, borderTop: '1px solid var(--border)', paddingTop: 12 }}>
        {!team ? (
          <span style={{ fontSize: 12, color: 'var(--muted)' }}>
            <Icon name="info" size={12} style={{ verticalAlign: '-1px', marginRight: 4 }} />
            Not a managed user — no disposition available.
          </span>
        ) : disabled ? (
          <StatusPill status="err" label="User disabled" />
        ) : (
          <>
            <button
              type="button"
              className={`btn btn-sm ${suggested === 'rate_limit' ? 'btn-primary' : ''}`}
              onClick={() => onRateLimit(team)}
            >
              <Icon name="gauge" size={13} /> Set rate limit
            </button>
            <button
              type="button"
              className="btn btn-sm"
              style={
                suggested === 'disable'
                  ? { background: 'var(--err)', color: '#fff', border: 'none' }
                  : { color: 'var(--err)' }
              }
              onClick={() => onDisable(team)}
            >
              <Icon name="pause" size={13} /> Disable user
            </button>
          </>
        )}
      </div>
    </div>
  )
}

export default function GovernancePage() {
  const qc = useQueryClient()
  const { push } = useToast()
  const [filters, setFilters] = useState<FilterValues>(DEFAULT_FILTERS)

  // Disposition modal state.
  const [rlTeam, setRlTeam] = useState<AdminTeam | null>(null)
  const [rlError, setRlError] = useState<string | undefined>()
  const [confirmDisable, setConfirmDisable] = useState<AdminTeam | null>(null)
  const [disableError, setDisableError] = useState<string | undefined>()

  const params = filterValuesToParams(filters)
  const { data, isLoading, error } = useQuery({
    queryKey: ['analytics', params],
    queryFn: () => api.analytics(params),
  })
  // Managed users, to resolve each flagged member's current enabled/limit state.
  const { data: teamsData } = useQuery({ queryKey: ['teams'], queryFn: api.teams })
  const teamById = new Map((teamsData?.data ?? []).map((t) => [t.id, t]))

  const rateLimitMutation = useMutation({
    mutationFn: ({ id, rate_limit }: { id: string; rate_limit: RateLimit | null }) =>
      api.updateTeam(id, { rate_limit }),
    onSuccess: (updated) => {
      void qc.invalidateQueries({ queryKey: ['teams'] })
      void qc.invalidateQueries({ queryKey: ['analytics'] })
      const has = updated.policy.rate_limit?.rpm != null || updated.policy.rate_limit?.tpm != null
      push(`Rate limit ${has ? 'applied' : 'removed'} for "${updated.id}"`, 'ok')
      setRlTeam(null)
    },
    onError: (err: unknown) => {
      setRlError(err instanceof Error ? err.message : 'Failed to update rate limit')
    },
  })

  const disableMutation = useMutation({
    mutationFn: (id: string) => api.updateTeam(id, { enabled: false }),
    onSuccess: (updated) => {
      void qc.invalidateQueries({ queryKey: ['teams'] })
      void qc.invalidateQueries({ queryKey: ['analytics'] })
      push(`User "${updated.id}" disabled`, 'ok')
      setConfirmDisable(null)
    },
    onError: (err: unknown) => {
      setDisableError(err instanceof Error ? err.message : 'Failed to disable user')
    },
  })

  const behavior = data?.behavior
  const members = behavior?.members ?? []
  const criticalCount = members.filter((m) => m.severity === 'critical').length

  return (
    <>
      <Topbar breadcrumbs={[{ label: 'Access' }, { label: 'Governance' }]} />
      <div className="page-pad">
        <div className="page-head">
          <h1 className="page-title">Governance</h1>
          <p className="page-sub">Behavior profiling flags waste-type abuse — loops, retry storms, idle spinning, off-hours automation — for you to review and act on.</p>
        </div>

        <div className="card" style={{ padding: '12px 16px', marginBottom: 16, display: 'flex', gap: 12, alignItems: 'center', flexWrap: 'wrap' }}>
          <FiltersBar values={filters} options={data?.filter_options} onChange={setFilters} />
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

        {data && !behavior && (
          <Empty
            icon="shield"
            title="Profiling not enabled"
            sub="Set profiling.enabled = true in the gateway config to collect baselines and surface abuse signals here."
          />
        )}

        {behavior && (
          <>
            <div style={{ display: 'grid', gridTemplateColumns: 'repeat(3, 1fr)', gap: 16, marginBottom: 16 }}>
              <Stat label="Evaluated users" value={behavior.evaluated.toLocaleString()} />
              <Stat label="Flagged" value={behavior.flagged.toLocaleString()} accent={behavior.flagged > 0 ? 'var(--warn)' : undefined} />
              <Stat label="Critical" value={criticalCount.toLocaleString()} accent={criticalCount > 0 ? 'var(--err)' : undefined} />
            </div>

            {members.length === 0 ? (
              <Empty
                icon="check"
                title="No abusive behavior detected"
                sub={`${behavior.evaluated} user${behavior.evaluated === 1 ? '' : 's'} evaluated this period — none tripped a threshold.`}
              />
            ) : (
              members.map((m) => (
                <MemberCard
                  key={m.id}
                  member={m}
                  team={teamById.get(m.id)}
                  onRateLimit={(t) => {
                    setRlError(undefined)
                    setRlTeam(t)
                  }}
                  onDisable={(t) => {
                    setDisableError(undefined)
                    setConfirmDisable(t)
                  }}
                />
              ))
            )}
          </>
        )}
      </div>

      {/* Rate-limit disposition — reuses the same editor as the Rate Limits page. */}
      {rlTeam && (
        <RateLimitEditor
          team={rlTeam}
          busy={rateLimitMutation.isPending}
          error={rlError}
          onCancel={() => setRlTeam(null)}
          onSubmit={(rate_limit) => {
            setRlError(undefined)
            rateLimitMutation.mutate({ id: rlTeam.id, rate_limit })
          }}
        />
      )}

      {/* Disable disposition — explicit confirm; hard-pauses the user. */}
      {confirmDisable && (
        <Modal
          open
          onClose={disableMutation.isPending ? () => {} : () => setConfirmDisable(null)}
          title={`Disable user · ${confirmDisable.id}`}
          width={440}
          footer={
            <>
              <button className="btn btn-sm" onClick={() => setConfirmDisable(null)} disabled={disableMutation.isPending}>
                Cancel
              </button>
              <button
                className="btn btn-sm"
                style={{ background: 'var(--err)', color: '#fff', border: 'none' }}
                disabled={disableMutation.isPending}
                onClick={() => {
                  setDisableError(undefined)
                  disableMutation.mutate(confirmDisable.id)
                }}
              >
                {disableMutation.isPending ? <span className="spinner" style={{ width: 12, height: 12 }} /> : null}
                Disable user
              </button>
            </>
          }
        >
          {disableError && (
            <div style={{ padding: '8px 12px', marginBottom: 14, borderRadius: 'var(--r-sm)', background: 'var(--err-soft)', color: 'var(--err)', fontSize: 13 }}>
              {disableError}
            </div>
          )}
          <p style={{ fontSize: 13, color: 'var(--ink-2)', lineHeight: 1.5 }}>
            All model requests from <strong>{confirmDisable.id}</strong> will be rejected until you re-enable the user on the Users page. Existing keys are not revoked.
          </p>
        </Modal>
      )}
    </>
  )
}
