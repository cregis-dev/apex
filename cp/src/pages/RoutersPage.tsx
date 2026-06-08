import { useEffect, useState } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import Topbar from '../components/Topbar.tsx'
import Empty from '../components/Empty.tsx'
import Icon from '../components/Icon.tsx'
import Modal from '../components/Modal.tsx'
import { useToast } from '../components/Toast.tsx'
import { api } from '../lib/api.ts'
import type {
  AdminChannel,
  AdminRouter,
  AdminTeam,
  CreateRouterRequest,
  RouterRule,
  RouterRuleInput,
  UpdateRouterRequest,
} from '../lib/types.ts'

const STRATEGIES = ['round_robin', 'random', 'priority'] as const
type Strategy = (typeof STRATEGIES)[number]

// ---------- form state ----------

// Monotonic id so React can keep rule rows stable across add/remove/reorder
// instead of falling back to array index (which remounts inputs and drops focus).
let ruleUidSeq = 0
function nextRuleUid(): string {
  ruleUidSeq += 1
  return `rule-${ruleUidSeq}`
}

interface RuleFormState {
  _uid: string
  models_csv: string
  channels: string[]
  strategy: Strategy
}

interface RouterFormState {
  name: string
  rules: RuleFormState[]
  fallback_channels: string[]
}

function emptyForm(): RouterFormState {
  return {
    name: '',
    rules: [{ _uid: nextRuleUid(), models_csv: '*', channels: [], strategy: 'round_robin' }],
    fallback_channels: [],
  }
}

function routerToForm(r: AdminRouter): RouterFormState {
  const rules: RuleFormState[] = r.rules.length > 0
    ? r.rules.map((rule) => ({
        _uid: nextRuleUid(),
        models_csv: rule.match.models.join(', '),
        channels: rule.channels.map((c) => c.name),
        strategy: (STRATEGIES.includes(rule.strategy as Strategy) ? rule.strategy : 'round_robin') as Strategy,
      }))
    : [{
        // Legacy router without explicit rules — surface its top-level channels
        // as a single * rule so the form is editable.
        _uid: nextRuleUid(),
        models_csv: '*',
        channels: (r.channels ?? []).map((c) => c.name),
        strategy: (STRATEGIES.includes((r.strategy ?? 'round_robin') as Strategy) ? (r.strategy ?? 'round_robin') : 'round_robin') as Strategy,
      }]
  return {
    name: r.name,
    rules,
    fallback_channels: r.fallback_channels ?? [],
  }
}

function ruleFromForm(rule: RuleFormState): RouterRuleInput {
  const models = rule.models_csv
    .split(',')
    .map((m) => m.trim())
    .filter(Boolean)
  return {
    models,
    channels: rule.channels.map((name) => ({ name })),
    strategy: rule.strategy,
  }
}

// ---------- helpers ----------

function ChannelChips({
  available,
  selected,
  onToggle,
}: {
  available: AdminChannel[]
  selected: string[]
  onToggle: (name: string) => void
}) {
  if (available.length === 0) {
    return (
      <div style={{ fontSize: 12, color: 'var(--muted)' }}>
        No channels are configured yet. Create channels first.
      </div>
    )
  }
  return (
    <div style={{ display: 'flex', flexWrap: 'wrap', gap: 6 }}>
      {available.map((ch) => {
        const active = selected.includes(ch.name)
        return (
          <button
            key={ch.name}
            type="button"
            onClick={() => onToggle(ch.name)}
            className="badge"
            style={{
              background: active ? 'var(--brand-soft)' : 'var(--surface-2)',
              color: active ? 'var(--brand-ink)' : 'var(--ink-2)',
              borderColor: active ? 'transparent' : 'var(--border)',
              cursor: 'pointer',
              height: 24, padding: '0 10px', fontSize: 12,
            }}
          >
            {active && <Icon name="check" size={11} />}
            {ch.name}
          </button>
        )
      })}
    </div>
  )
}

function Field({ label, children, hint }: { label: string; children: React.ReactNode; hint?: string }) {
  return (
    <div>
      <div style={{ fontSize: 12, fontWeight: 500, color: 'var(--ink-2)', marginBottom: 6 }}>
        {label}
      </div>
      {children}
      {hint && (
        <div style={{ fontSize: 11, color: 'var(--muted)', marginTop: 4 }}>{hint}</div>
      )}
    </div>
  )
}

// ---------- modal ----------

interface RouterEditorProps {
  open: boolean
  mode: 'create' | 'edit'
  initial: RouterFormState
  channels: AdminChannel[]
  busy: boolean
  error?: string
  onCancel: () => void
  onSubmit: (form: RouterFormState) => void
}

function RouterEditor({ open, mode, initial, channels, busy, error, onCancel, onSubmit }: RouterEditorProps) {
  const [form, setForm] = useState<RouterFormState>(initial)
  // Reset the form whenever the modal (re)opens with new initial values.
  useEffect(() => {
    if (open) setForm(initial)
  }, [open, initial])

  const setRule = (idx: number, patch: Partial<RuleFormState>) => {
    setForm((f) => ({
      ...f,
      rules: f.rules.map((r, i) => (i === idx ? { ...r, ...patch } : r)),
    }))
  }
  const addRule = () => {
    setForm((f) => ({
      ...f,
      rules: [...f.rules, { _uid: nextRuleUid(), models_csv: '', channels: [], strategy: 'round_robin' }],
    }))
  }
  const removeRule = (idx: number) => {
    setForm((f) => ({
      ...f,
      rules: f.rules.length === 1 ? f.rules : f.rules.filter((_, i) => i !== idx),
    }))
  }

  const idInvalid = mode === 'create' && !form.name.trim()
  const ruleInvalid = form.rules.some((r) => r.channels.length === 0 || !r.models_csv.trim())

  return (
    <Modal
      open={open}
      onClose={busy ? () => {} : onCancel}
      title={mode === 'create' ? 'Create router' : `Edit router · ${initial.name}`}
      width={620}
      footer={
        <>
          <button className="btn btn-sm" onClick={onCancel} disabled={busy}>Cancel</button>
          <button
            className="btn btn-primary btn-sm"
            disabled={busy || idInvalid || ruleInvalid}
            onClick={() => onSubmit(form)}
          >
            {busy ? <span className="spinner" style={{ width: 12, height: 12 }} /> : null}
            {mode === 'create' ? 'Create router' : 'Save changes'}
          </button>
        </>
      }
    >
      {error && (
        <div style={{ padding: '8px 12px', marginBottom: 14, borderRadius: 'var(--r-sm)', background: 'var(--err-soft)', color: 'var(--err)', fontSize: 13 }}>
          {error}
        </div>
      )}

      <div style={{ display: 'grid', gap: 14 }}>
        <Field
          label="Router name"
          hint={mode === 'edit' ? 'Router name cannot be changed after creation.' : undefined}
        >
          <input
            className="input"
            value={form.name}
            onChange={(e) => setForm((f) => ({ ...f, name: e.target.value }))}
            placeholder="e.g. all-llms"
            disabled={mode === 'edit'}
            style={{ width: '100%' }}
          />
        </Field>

        <div>
          <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', marginBottom: 6 }}>
            <span style={{ fontSize: 12, fontWeight: 500, color: 'var(--ink-2)' }}>
              Rules ({form.rules.length})
            </span>
            <button className="btn btn-sm" onClick={addRule} type="button">
              <Icon name="plus" size={12} /> Add rule
            </button>
          </div>
          <div style={{ display: 'grid', gap: 10 }}>
            {form.rules.map((rule, idx) => (
              <div
                key={rule._uid}
                className="card"
                style={{ padding: '12px 14px', display: 'grid', gap: 10 }}
              >
                <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between' }}>
                  <span style={{ fontSize: 11, fontWeight: 600, textTransform: 'uppercase', letterSpacing: '0.04em', color: 'var(--muted)' }}>
                    Rule #{idx + 1}
                  </span>
                  <button
                    className="btn btn-ghost btn-sm"
                    style={{ padding: '0 6px', color: 'var(--err)' }}
                    onClick={() => removeRule(idx)}
                    disabled={form.rules.length === 1}
                    title={form.rules.length === 1 ? 'A router must have at least one rule' : 'Remove this rule'}
                  >
                    <Icon name="trash" size={12} />
                  </button>
                </div>
                <Field
                  label="Match models"
                  hint="Comma-separated. Supports glob: * matches all, deepseek-* matches a prefix."
                >
                  <input
                    className="input"
                    value={rule.models_csv}
                    onChange={(e) => setRule(idx, { models_csv: e.target.value })}
                    placeholder="gpt-4*, deepseek-*"
                    style={{ width: '100%', fontFamily: 'var(--font-mono)' }}
                  />
                </Field>
                <Field label={`Channels · ${rule.channels.length} selected`}>
                  <ChannelChips
                    available={channels}
                    selected={rule.channels}
                    onToggle={(name) => setRule(idx, {
                      channels: rule.channels.includes(name)
                        ? rule.channels.filter((c) => c !== name)
                        : [...rule.channels, name],
                    })}
                  />
                </Field>
                <Field label="Strategy">
                  <select
                    className="select"
                    value={rule.strategy}
                    onChange={(e) => setRule(idx, { strategy: e.target.value as Strategy })}
                    style={{ width: 200 }}
                  >
                    {STRATEGIES.map((s) => <option key={s} value={s}>{s.replace('_', '-')}</option>)}
                  </select>
                </Field>
              </div>
            ))}
          </div>
        </div>

        <Field label="Fallback channels (optional)" hint="Tried in order if every primary channel fails.">
          <ChannelChips
            available={channels}
            selected={form.fallback_channels}
            onToggle={(name) =>
              setForm((f) => ({
                ...f,
                fallback_channels: f.fallback_channels.includes(name)
                  ? f.fallback_channels.filter((n) => n !== name)
                  : [...f.fallback_channels, name],
              }))
            }
          />
        </Field>
      </div>
    </Modal>
  )
}

// ---------- detail (read-only) view ----------

function RouterDetail({ router }: { router: AdminRouter }) {
  // A router is a list of independent rules — each rule has its own match
  // models, channels and strategy. Render it that way. (Earlier this view
  // collapsed everything into one global strategy + a flattened channel list,
  // which double-counted a channel reused across rules and hid per-rule
  // strategies.) Legacy routers without explicit rules are normalized into a
  // single synthetic "*" rule so the layout stays uniform.
  const rules: RouterRule[] = router.rules.length > 0
    ? router.rules
    : [{
        match: { models: ['*'] },
        channels: router.channels ?? [],
        strategy: router.strategy ?? 'round_robin',
      }]
  const fallback = router.fallback_channels ?? []

  return (
    <div className="card" style={{ padding: '18px 20px', flex: 1 }}>
      <div style={{ marginBottom: 20 }}>
        <div style={{ fontSize: 16, fontWeight: 600 }}>{router.name}</div>
        <div style={{ fontSize: 12, fontFamily: 'var(--font-mono)', color: 'var(--muted)', marginTop: 2 }}>
          {rules.length} rule{rules.length === 1 ? '' : 's'}
        </div>
      </div>

      <div style={{ display: 'flex', flexDirection: 'column', gap: 14 }}>
        {rules.map((rule, i) => (
          <div key={i} className="card" style={{ padding: '14px 16px', background: 'var(--surface-2)' }}>
            <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', marginBottom: 12 }}>
              <span style={{ fontSize: 11, fontWeight: 600, textTransform: 'uppercase', letterSpacing: '0.04em', color: 'var(--muted)' }}>
                Rule #{i + 1}
              </span>
              <span className="badge badge-brand">{rule.strategy.replace('_', '-')}</span>
            </div>

            <div className="section-title" style={{ margin: '0 0 6px' }}>Match models</div>
            <div style={{ fontFamily: 'var(--font-mono)', fontSize: 12, color: 'var(--brand-ink)', marginBottom: 14 }}>
              {rule.match.models.length > 0 ? rule.match.models.join(', ') : '*'}
            </div>

            <div className="section-title" style={{ margin: '0 0 8px' }}>Channels ({rule.channels.length})</div>
            {rule.channels.length === 0 ? (
              <div style={{ color: 'var(--muted)', fontSize: 13 }}>No channels assigned.</div>
            ) : (
              <div style={{ display: 'flex', flexDirection: 'column', gap: 6 }}>
                {rule.channels.map((ch, j) => (
                  <div key={`${ch.name}-${j}`} className="card" style={{ padding: '8px 12px', display: 'flex', alignItems: 'center', gap: 12, background: 'var(--surface)' }}>
                    <span style={{ width: 20, height: 20, borderRadius: 'var(--r-xs)', background: 'var(--surface-2)', border: '1px solid var(--border)', display: 'flex', alignItems: 'center', justifyContent: 'center', fontSize: 11, fontWeight: 600, color: 'var(--muted)', flexShrink: 0 }}>
                      {j + 1}
                    </span>
                    <span style={{ flex: 1, fontSize: 13, fontWeight: 500 }}>{ch.name}</span>
                    {rule.strategy === 'weighted' && (
                      <span style={{ fontSize: 12, fontFamily: 'var(--font-mono)', color: 'var(--muted)' }}>weight: {ch.weight}</span>
                    )}
                  </div>
                ))}
              </div>
            )}
          </div>
        ))}
      </div>

      {fallback.length > 0 && (
        <>
          <div className="section-title" style={{ marginTop: 20 }}>Fallback channels</div>
          <div style={{ display: 'flex', gap: 8, flexWrap: 'wrap' }}>
            {fallback.map((c) => (
              <span key={c} className="badge">{c}</span>
            ))}
          </div>
        </>
      )}
    </div>
  )
}

// ---------- page ----------

export default function RoutersPage() {
  const qc = useQueryClient()
  const { push } = useToast()
  const { data, isLoading, error } = useQuery({
    queryKey: ['routers'],
    queryFn: api.routers,
  })
  const { data: channelsData } = useQuery({
    queryKey: ['channels'],
    queryFn: api.channels,
  })
  const { data: teamsData } = useQuery({
    queryKey: ['teams'],
    queryFn: api.teams,
  })

  const routers = data?.data ?? []
  const channels = channelsData?.data ?? []
  const teams: AdminTeam[] = teamsData?.data ?? []

  const [selected, setSelected] = useState<string | null>(null)
  const active = routers.find((r) => r.name === selected) ?? routers[0] ?? null

  const [editorOpen, setEditorOpen] = useState(false)
  const [editorMode, setEditorMode] = useState<'create' | 'edit'>('create')
  const [editorInitial, setEditorInitial] = useState<RouterFormState>(emptyForm())
  const [editorError, setEditorError] = useState<string | undefined>()
  const [editingName, setEditingName] = useState<string | null>(null)
  const [pendingDelete, setPendingDelete] = useState<AdminRouter | null>(null)
  const [deleteError, setDeleteError] = useState<string | undefined>()

  const invalidate = () => {
    void qc.invalidateQueries({ queryKey: ['routers'] })
  }

  const createMutation = useMutation({
    mutationFn: (body: CreateRouterRequest) => api.createRouter(body),
    onSuccess: (created) => {
      invalidate()
      setEditorOpen(false)
      setSelected(created.name)
      push(`Router "${created.name}" created`, 'ok')
    },
    onError: (err: unknown) => {
      setEditorError(err instanceof Error ? err.message : 'Failed to create router')
    },
  })

  const updateMutation = useMutation({
    mutationFn: ({ name, body }: { name: string; body: UpdateRouterRequest }) =>
      api.updateRouter(name, body),
    onSuccess: (updated) => {
      invalidate()
      setEditorOpen(false)
      push(`Router "${updated.name}" updated`, 'ok')
    },
    onError: (err: unknown) => {
      setEditorError(err instanceof Error ? err.message : 'Failed to update router')
    },
  })

  const deleteMutation = useMutation({
    mutationFn: (name: string) => api.deleteRouter(name),
    onSuccess: (_, name) => {
      invalidate()
      setPendingDelete(null)
      setDeleteError(undefined)
      if (selected === name) setSelected(null)
      push(`Router "${name}" deleted`, 'ok')
    },
    onError: (err: unknown) => {
      setDeleteError(err instanceof Error ? err.message : 'Failed to delete router')
    },
  })

  function openCreate() {
    setEditorMode('create')
    setEditorInitial(emptyForm())
    setEditingName(null)
    setEditorError(undefined)
    setEditorOpen(true)
  }

  function openEdit(r: AdminRouter) {
    setEditorMode('edit')
    setEditorInitial(routerToForm(r))
    setEditingName(r.name)
    setEditorError(undefined)
    setEditorOpen(true)
  }

  function submitEditor(form: RouterFormState) {
    setEditorError(undefined)
    const rules = form.rules.map(ruleFromForm)
    if (editorMode === 'create') {
      createMutation.mutate({
        name: form.name.trim(),
        rules,
        fallback_channels: form.fallback_channels,
      })
    } else if (editingName) {
      updateMutation.mutate({
        name: editingName,
        body: { rules, fallback_channels: form.fallback_channels },
      })
    }
  }

  const teamsReferencingPending = pendingDelete
    ? teams.filter((t) => t.policy.allowed_routers.includes(pendingDelete.name))
    : []

  return (
    <>
      <Topbar
        breadcrumbs={[{ label: 'Configure' }, { label: 'Routers' }]}
        actions={
          <>
            <button className="btn btn-sm" onClick={() => void qc.invalidateQueries({ queryKey: ['routers'] })}>
              <Icon name="refresh" size={13} /> Refresh
            </button>
            <button className="btn btn-primary btn-sm" onClick={openCreate}>
              <Icon name="plus" size={13} /> New router
            </button>
          </>
        }
      />
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
          <Empty
            icon="route"
            title="No routers configured"
            sub="Click ‘New router’ to define how teams should route to your channels."
          />
        )}

        {routers.length > 0 && (
          <div style={{ display: 'grid', gridTemplateColumns: '280px 1fr', gap: 16 }}>
            <div className="card" style={{ padding: 8, alignSelf: 'start' }}>
              {routers.map((r) => {
                const isActive = (active?.name ?? '') === r.name
                const ruleCount = r.rules.length
                const channelCount = r.rules.flatMap((ru) => ru.channels).length || (r.channels ?? []).length
                return (
                  <div
                    key={r.name}
                    style={{
                      display: 'flex', alignItems: 'center', gap: 4,
                      borderRadius: 'var(--r-sm)',
                      background: isActive ? 'var(--bg-soft)' : 'transparent',
                      border: isActive ? '1px solid var(--border)' : '1px solid transparent',
                      marginBottom: 2, padding: 2,
                    }}
                  >
                    <button
                      onClick={() => setSelected(r.name)}
                      style={{
                        flex: 1, textAlign: 'left',
                        background: 'transparent', border: 'none',
                        padding: '8px 10px', borderRadius: 'var(--r-sm)',
                        cursor: 'pointer',
                      }}
                    >
                      <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
                        <span className={`dot dot-${isActive ? 'ok' : 'muted'}`} />
                        <span style={{ fontWeight: 500, fontSize: 13 }}>{r.name}</span>
                      </div>
                      <div style={{ fontSize: 12, color: 'var(--muted)', marginTop: 2, fontFamily: 'var(--font-mono)' }}>
                        {ruleCount} rule{ruleCount === 1 ? '' : 's'} · {channelCount} ch
                      </div>
                    </button>
                    <button
                      className="btn btn-ghost btn-sm"
                      style={{ padding: '0 6px', flexShrink: 0 }}
                      onClick={() => openEdit(r)}
                      title="Edit"
                    >
                      <Icon name="edit" size={12} />
                    </button>
                    <button
                      className="btn btn-ghost btn-sm"
                      style={{ padding: '0 6px', flexShrink: 0, color: 'var(--err)' }}
                      onClick={() => { setPendingDelete(r); setDeleteError(undefined) }}
                      title="Delete"
                    >
                      <Icon name="trash" size={12} />
                    </button>
                  </div>
                )
              })}
            </div>

            {active && <RouterDetail router={active} />}
          </div>
        )}
      </div>

      <RouterEditor
        open={editorOpen}
        mode={editorMode}
        initial={editorInitial}
        channels={channels}
        busy={createMutation.isPending || updateMutation.isPending}
        error={editorError}
        onCancel={() => setEditorOpen(false)}
        onSubmit={submitEditor}
      />

      <Modal
        open={!!pendingDelete}
        onClose={() => {
          if (!deleteMutation.isPending) {
            setPendingDelete(null)
            setDeleteError(undefined)
          }
        }}
        title="Delete router"
        width={480}
        footer={
          <>
            <button
              className="btn btn-sm"
              onClick={() => { setPendingDelete(null); setDeleteError(undefined) }}
              disabled={deleteMutation.isPending}
            >
              Cancel
            </button>
            <button
              className="btn btn-sm"
              style={{ background: 'var(--err)', color: '#fff', borderColor: 'transparent' }}
              disabled={deleteMutation.isPending || teamsReferencingPending.length > 0}
              onClick={() => pendingDelete && deleteMutation.mutate(pendingDelete.name)}
              title={teamsReferencingPending.length > 0 ? 'Remove the router from all teams’ allowed_routers first' : 'Delete this router'}
            >
              {deleteMutation.isPending
                ? <span className="spinner" style={{ width: 12, height: 12 }} />
                : <Icon name="trash" size={13} />}
              Delete router
            </button>
          </>
        }
      >
        {pendingDelete && (
          <div style={{ fontSize: 13, color: 'var(--ink-2)' }}>
            <p style={{ marginTop: 0 }}>
              You're about to delete router <strong>{pendingDelete.name}</strong>.
            </p>
            {teamsReferencingPending.length > 0 ? (
              <div style={{
                padding: '10px 12px', borderRadius: 'var(--r-sm)',
                background: 'var(--warn-soft)', color: 'oklch(0.42 0.1 70)',
                fontSize: 12, marginTop: 10,
              }}>
                <div style={{ fontWeight: 600, marginBottom: 6 }}>
                  Blocked — still referenced by {teamsReferencingPending.length} team{teamsReferencingPending.length === 1 ? '' : 's'}:
                </div>
                <div style={{ display: 'flex', gap: 4, flexWrap: 'wrap' }}>
                  {teamsReferencingPending.map((t) => (
                    <span key={t.id} className="badge">{t.id}</span>
                  ))}
                </div>
                <div style={{ marginTop: 8 }}>
                  Remove it from each team's allowed_routers in Teams page first, then come back.
                </div>
              </div>
            ) : (
              <p>
                Any team that still has this router in <code>allowed_routers</code> will start
                receiving routing errors. The gateway will refuse the delete if such teams exist.
              </p>
            )}
            {deleteError && (
              <div style={{ marginTop: 10, padding: '8px 12px', borderRadius: 'var(--r-sm)', background: 'var(--err-soft)', color: 'var(--err)', fontSize: 12 }}>
                {deleteError}
              </div>
            )}
          </div>
        )}
      </Modal>
    </>
  )
}
