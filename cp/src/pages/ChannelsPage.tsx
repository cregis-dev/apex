import { useEffect, useState } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import Topbar from '../components/Topbar.tsx'
import ProviderMark from '../components/ProviderMark.tsx'
import Icon from '../components/Icon.tsx'
import Empty from '../components/Empty.tsx'
import Modal from '../components/Modal.tsx'
import { useToast } from '../components/Toast.tsx'
import { api } from '../lib/api.ts'
import type {
  AdminChannel,
  CreateChannelRequest,
  PricingRule,
  ProviderTemplate,
  ProviderType,
  UpdateChannelRequest,
} from '../lib/types.ts'

const PROVIDER_TYPES: ProviderType[] = [
  'openai', 'anthropic', 'gemini', 'custom_dual',
  'deepseek', 'moonshot', 'minimax', 'ollama',
  'jina', 'openrouter', 'zai',
]

function EndpointLine({ label, url }: { label: string; url: string }) {
  return (
    <div
      title={`${label}: ${url}`}
      style={{
        display: 'flex', alignItems: 'center', gap: 6, minWidth: 0,
        fontFamily: 'var(--font-mono)', fontSize: 12,
      }}
    >
      <span
        style={{
          flex: '0 0 auto',
          fontFamily: 'var(--font-sans)',
          fontSize: 10, fontWeight: 600,
          textTransform: 'uppercase', letterSpacing: '0.04em',
          color: 'var(--muted)',
          width: 68,
        }}
      >
        {label}
      </span>
      <span
        style={{
          flex: '1 1 0', minWidth: 0,
          color: 'var(--ink-2)',
          overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap',
        }}
      >
        {url}
      </span>
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

/**
 * One `model_map` entry as edited in the form. Kept as an ordered array rather
 * than an object so rows stay put while typing (and half-filled rows survive).
 */
interface ModelMapRow { from: string; to: string }

function modelMapToRows(map: Record<string, string> | null | undefined): ModelMapRow[] {
  return Object.entries(map ?? {}).map(([from, to]) => ({ from, to }))
}

/**
 * Drop blank/half-filled rows; later duplicates overwrite earlier ones.
 *
 * Built via `Object.fromEntries` rather than by assignment: `out[from] = to`
 * on a plain object hits the inherited accessor when `from` is `__proto__`,
 * so that row would vanish — and since the server round-trips such a key
 * fine (`JSON.parse` gives it as an own property), the rebuilt map would then
 * read as "changed to empty" and clear the channel's mapping on any save.
 */
function rowsToModelMap(rows: ModelMapRow[]): Record<string, string> {
  return Object.fromEntries(
    rows
      .map((row) => [row.from.trim(), row.to.trim()])
      .filter(([from, to]) => from && to),
  )
}

/** Order-insensitive equality, so reordering rows alone isn't a "change". */
function sameModelMap(a: Record<string, string>, b: Record<string, string>): boolean {
  const ka = Object.keys(a)
  const kb = Object.keys(b)
  return ka.length === kb.length && ka.every((k) => b[k] === a[k])
}

/** Source model names typed more than once — one would silently shadow the other. */
function duplicateSources(rows: ModelMapRow[]): string[] {
  const seen = new Set<string>()
  const dupes = new Set<string>()
  for (const row of rows) {
    const from = row.from.trim()
    if (!from) continue
    if (seen.has(from)) dupes.add(from)
    seen.add(from)
  }
  return [...dupes]
}

/** A row with exactly one side filled in — almost certainly unfinished. */
function hasIncompleteRow(rows: ModelMapRow[]): boolean {
  return rows.some((r) => !!r.from.trim() !== !!r.to.trim())
}

interface ChannelFormState {
  name: string
  provider_type: ProviderType
  base_url: string
  anthropic_base_url: string
  api_key: string
  keep_existing_key: boolean
  /** Selected pricing rule name, or '' for untracked. */
  pricing: string
  model_map: ModelMapRow[]
}

function emptyForm(): ChannelFormState {
  return {
    name: '',
    provider_type: 'openai',
    base_url: '',
    anthropic_base_url: '',
    api_key: '',
    keep_existing_key: false,
    pricing: '',
    model_map: [],
  }
}

function channelToForm(ch: AdminChannel): ChannelFormState {
  return {
    name: ch.name,
    provider_type: ch.provider_type,
    base_url: ch.base_url,
    anthropic_base_url: ch.anthropic_base_url ?? '',
    api_key: '',
    keep_existing_key: true,
    pricing: ch.pricing ?? '',
    model_map: modelMapToRows(ch.model_map),
  }
}

interface ChannelEditorProps {
  open: boolean
  mode: 'create' | 'edit'
  initial: ChannelFormState
  templates: ProviderTemplate[]
  rules: PricingRule[]
  busy: boolean
  error?: string
  onCancel: () => void
  onSubmit: (form: ChannelFormState) => void
}

function ChannelEditor({ open, mode, initial, templates, rules, busy, error, onCancel, onSubmit }: ChannelEditorProps) {
  const [form, setForm] = useState<ChannelFormState>(initial)

  // Reset the form whenever the modal (re)opens with new initial values.
  useEffect(() => {
    if (open) setForm(initial)
  }, [open, initial])

  const currentTemplate = templates.find((t) => t.provider_type === form.provider_type)

  // Selecting a provider pre-fills its default endpoints (from providers.json).
  // Create: always overwrite. Edit: only fill empty fields, never clobber a
  // custom URL already set on the channel.
  function handleProviderChange(pt: ProviderType) {
    const tpl = templates.find((t) => t.provider_type === pt)
    setForm((f) => {
      if (!tpl) return { ...f, provider_type: pt }
      if (mode === 'create') {
        return {
          ...f,
          provider_type: pt,
          base_url: tpl.base_url,
          anthropic_base_url: tpl.anthropic_base_url ?? '',
        }
      }
      return {
        ...f,
        provider_type: pt,
        base_url: f.base_url.trim() ? f.base_url : tpl.base_url,
        anthropic_base_url: f.anthropic_base_url.trim() ? f.anthropic_base_url : (tpl.anthropic_base_url ?? ''),
      }
    })
  }

  function applyDefaults() {
    if (!currentTemplate) return
    setForm((f) => ({
      ...f,
      base_url: currentTemplate.base_url,
      anthropic_base_url: currentTemplate.anthropic_base_url ?? '',
    }))
  }

  const setMapRow = (i: number, k: keyof ModelMapRow, v: string) =>
    setForm((f) => ({
      ...f,
      model_map: f.model_map.map((row, idx) => (idx === i ? { ...row, [k]: v } : row)),
    }))
  const addMapRow = () =>
    setForm((f) => ({ ...f, model_map: [...f.model_map, { from: '', to: '' }] }))
  const removeMapRow = (i: number) =>
    setForm((f) => ({ ...f, model_map: f.model_map.filter((_, idx) => idx !== i) }))

  const idInvalid = mode === 'create' && !form.name.trim()
  const baseInvalid = !form.base_url.trim()
  const keyInvalid = mode === 'create'
    ? !form.api_key.trim()
    : !form.keep_existing_key && !form.api_key.trim()
  const mapDupes = duplicateSources(form.model_map)
  // Block on ambiguity/unfinished input rather than silently dropping a mapping.
  const mapInvalid = mapDupes.length > 0 || hasIncompleteRow(form.model_map)

  return (
    <Modal
      open={open}
      onClose={busy ? () => {} : onCancel}
      title={mode === 'create' ? 'Create channel' : `Edit channel · ${initial.name}`}
      width={560}
      footer={
        <>
          <button className="btn btn-sm" onClick={onCancel} disabled={busy}>Cancel</button>
          <button
            className="btn btn-primary btn-sm"
            disabled={busy || idInvalid || baseInvalid || keyInvalid || mapInvalid}
            onClick={() => onSubmit(form)}
          >
            {busy ? <span className="spinner" style={{ width: 12, height: 12 }} /> : null}
            {mode === 'create' ? 'Create channel' : 'Save changes'}
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
          label="Channel name"
          hint={mode === 'edit' ? 'Channel name cannot be changed after creation.' : undefined}
        >
          <input
            className="input"
            value={form.name}
            onChange={(e) => setForm((f) => ({ ...f, name: e.target.value }))}
            placeholder="e.g. openai-prod"
            disabled={mode === 'edit'}
            style={{ width: '100%' }}
          />
        </Field>

        <Field
          label="Provider type"
          hint="Selecting a provider fills in its default endpoints below — you can still edit them."
        >
          <select
            className="select"
            value={form.provider_type}
            onChange={(e) => handleProviderChange(e.target.value as ProviderType)}
            style={{ width: '100%' }}
          >
            {PROVIDER_TYPES.map((p) => <option key={p} value={p}>{p}</option>)}
          </select>
        </Field>

        <Field label="Base URL (OpenAI-compatible)">
          <input
            className="input"
            value={form.base_url}
            onChange={(e) => setForm((f) => ({ ...f, base_url: e.target.value }))}
            placeholder={currentTemplate?.base_url || 'https://api.example.com/v1'}
            style={{ width: '100%', fontFamily: 'var(--font-mono)' }}
          />
          {currentTemplate && (form.base_url !== currentTemplate.base_url
            || form.anthropic_base_url !== (currentTemplate.anthropic_base_url ?? '')) && (
            <button
              type="button"
              onClick={applyDefaults}
              style={{
                marginTop: 6, padding: 0, border: 'none', background: 'none',
                color: 'var(--brand)', fontSize: 11, cursor: 'pointer',
              }}
            >
              Reset to {form.provider_type} defaults
            </button>
          )}
        </Field>

        <Field
          label="Anthropic base URL (optional)"
          hint="Set if this channel also serves /v1/messages on a separate URL"
        >
          <input
            className="input"
            value={form.anthropic_base_url}
            onChange={(e) => setForm((f) => ({ ...f, anthropic_base_url: e.target.value }))}
            placeholder={currentTemplate?.anthropic_base_url || 'https://api.example.com/anthropic'}
            style={{ width: '100%', fontFamily: 'var(--font-mono)' }}
          />
        </Field>

        <Field
          label={mode === 'edit' ? 'API key' : 'API key (upstream secret)'}
          hint={mode === 'edit' && form.keep_existing_key
            ? 'Leave as-is to keep the current key. Toggle below to rotate.'
            : undefined}
        >
          {mode === 'edit' && (
            <label style={{ display: 'flex', alignItems: 'center', gap: 6, marginBottom: 8, fontSize: 12, color: 'var(--ink-2)', cursor: 'pointer' }}>
              <input
                type="checkbox"
                checked={!form.keep_existing_key}
                onChange={(e) => setForm((f) => ({ ...f, keep_existing_key: !e.target.checked, api_key: '' }))}
              />
              Replace the current key
            </label>
          )}
          {(mode === 'create' || !form.keep_existing_key) && (
            <input
              className="input"
              type="password"
              value={form.api_key}
              onChange={(e) => setForm((f) => ({ ...f, api_key: e.target.value }))}
              placeholder="sk-…"
              style={{ width: '100%', fontFamily: 'var(--font-mono)' }}
            />
          )}
        </Field>

        <Field
          label="Pricing rule"
          hint="How this channel is billed for the dashboard cost view. Manage rules on the Pricing page."
        >
          <select
            className="select"
            value={form.pricing}
            onChange={(e) => setForm((f) => ({ ...f, pricing: e.target.value }))}
            style={{ width: '100%' }}
          >
            <option value="">None (untracked)</option>
            {rules.map((r) => (
              <option key={r.name} value={r.name}>
                {r.name} · {r.type === 'subscription' ? `subscription $${r.monthly_fee ?? 0}/mo` : 'pay-as-you-go'}
              </option>
            ))}
          </select>
          {form.pricing && !rules.some((r) => r.name === form.pricing) && (
            <div style={{ fontSize: 11, color: 'var(--warn)', marginTop: 4 }}>
              Rule "{form.pricing}" no longer exists — pick another or create it on the Pricing page.
            </div>
          )}
        </Field>

        <Field
          label="Model mapping (optional)"
          hint="Rewrites the model name before the request leaves for this channel. Exact match — no wildcards; unlisted models pass through untouched."
        >
          {form.model_map.length > 0 && (
            <div
              style={{
                display: 'grid', gridTemplateColumns: '1fr 1fr 28px', gap: 8,
                fontSize: 10, color: 'var(--muted)',
                textTransform: 'uppercase', letterSpacing: '0.04em',
                padding: '0 2px', marginBottom: 6,
              }}
            >
              <span>Requested model</span><span>Upstream model</span><span />
            </div>
          )}
          <div style={{ display: 'grid', gap: 8 }}>
            {form.model_map.map((row, i) => {
              const dupe = !!row.from.trim() && mapDupes.includes(row.from.trim())
              return (
                <div key={i} style={{ display: 'grid', gridTemplateColumns: '1fr 1fr 28px', gap: 8, alignItems: 'center' }}>
                  <input
                    className="input btn-sm"
                    value={row.from}
                    onChange={(e) => setMapRow(i, 'from', e.target.value)}
                    placeholder="claude-sonnet-4"
                    style={{
                      height: 30, fontFamily: 'var(--font-mono)', fontSize: 12,
                      borderColor: dupe ? 'var(--err)' : undefined,
                    }}
                  />
                  <input
                    className="input btn-sm"
                    value={row.to}
                    onChange={(e) => setMapRow(i, 'to', e.target.value)}
                    placeholder="MiniMax-M2"
                    style={{ height: 30, fontFamily: 'var(--font-mono)', fontSize: 12 }}
                  />
                  <button
                    className="btn btn-ghost btn-sm"
                    title="Remove mapping"
                    onClick={() => removeMapRow(i)}
                    style={{ padding: 0, height: 30, color: 'var(--err)' }}
                  >
                    <Icon name="trash" size={12} />
                  </button>
                </div>
              )
            })}
          </div>
          <button
            className="btn btn-sm"
            onClick={addMapRow}
            style={{ marginTop: form.model_map.length > 0 ? 8 : 0 }}
          >
            <Icon name="plus" size={12} /> Add mapping
          </button>
          {mapDupes.length > 0 && (
            <div style={{ fontSize: 11, color: 'var(--err)', marginTop: 6 }}>
              Duplicate requested model: {mapDupes.join(', ')} — each one can map to a single upstream model.
            </div>
          )}
          {mapDupes.length === 0 && hasIncompleteRow(form.model_map) && (
            <div style={{ fontSize: 11, color: 'var(--warn)', marginTop: 6 }}>
              Fill in both sides of every mapping, or remove the unfinished row.
            </div>
          )}
        </Field>
      </div>
    </Modal>
  )
}

export default function ChannelsPage() {
  const { push } = useToast()
  const qc = useQueryClient()
  const { data, isLoading, error, refetch } = useQuery({
    queryKey: ['channels'],
    queryFn: api.channels,
    // `model_map` is keyed by arbitrary model names. React Query's structural
    // sharing rebuilds cached objects by assignment, which silently drops a
    // `__proto__` key on every refetch after the first — the form would then
    // reopen without that row and save the mapping away. Keep the parsed
    // response verbatim; this list is small, so the lost referential
    // stability costs nothing.
    structuralSharing: false,
  })
  const { data: keysData } = useQuery({
    queryKey: ['channels', 'api_keys'],
    queryFn: api.channelApiKeys,
    staleTime: 30_000,
  })
  const { data: templatesData } = useQuery({
    queryKey: ['provider-templates'],
    queryFn: api.providerTemplates,
    staleTime: 5 * 60_000,
  })
  const { data: pricingData } = useQuery({
    queryKey: ['pricing'],
    queryFn: api.pricing,
    staleTime: 30_000,
  })

  const channels = data?.data ?? []
  const keyByName = new Map((keysData?.data ?? []).map((e) => [e.name, e.api_key]))
  const templates = templatesData?.data ?? []
  const rules = pricingData?.rules ?? []
  const ruleByName = new Map(rules.map((r) => [r.name, r]))

  const [editorOpen, setEditorOpen] = useState(false)
  const [editorMode, setEditorMode] = useState<'create' | 'edit'>('create')
  const [editorInitial, setEditorInitial] = useState<ChannelFormState>(emptyForm())
  const [editorError, setEditorError] = useState<string | undefined>()
  const [editingName, setEditingName] = useState<string | null>(null)
  const [pendingDelete, setPendingDelete] = useState<AdminChannel | null>(null)
  const [deleteError, setDeleteError] = useState<string | undefined>()

  const invalidate = () => {
    // Prefix match also covers ['channels','api_keys'], but be explicit so the
    // masked-key column always refreshes after a mutation.
    void qc.invalidateQueries({ queryKey: ['channels'] })
    void qc.invalidateQueries({ queryKey: ['channels', 'api_keys'] })
  }

  const createMutation = useMutation({
    mutationFn: (body: CreateChannelRequest) => api.createChannel(body),
    onSuccess: (created) => {
      invalidate()
      setEditorOpen(false)
      push(`Channel "${created.name}" created`, 'ok')
    },
    onError: (err: unknown) => {
      setEditorError(err instanceof Error ? err.message : 'Failed to create channel')
    },
  })

  const updateMutation = useMutation({
    mutationFn: ({ name, body }: { name: string; body: UpdateChannelRequest }) =>
      api.updateChannel(name, body),
    onSuccess: (updated) => {
      invalidate()
      setEditorOpen(false)
      push(`Channel "${updated.name}" updated`, 'ok')
    },
    onError: (err: unknown) => {
      setEditorError(err instanceof Error ? err.message : 'Failed to update channel')
    },
  })

  const deleteMutation = useMutation({
    mutationFn: (name: string) => api.deleteChannel(name),
    onSuccess: (_, name) => {
      invalidate()
      setPendingDelete(null)
      setDeleteError(undefined)
      push(`Channel "${name}" deleted`, 'ok')
    },
    onError: (err: unknown) => {
      setDeleteError(err instanceof Error ? err.message : 'Failed to delete channel')
    },
  })

  const copy = (text: string, label: string) => {
    void navigator.clipboard.writeText(text).then(() => push(label, 'ok'))
  }

  function openCreate() {
    // Pre-fill the default provider's endpoints from providers.json.
    const base = emptyForm()
    const tpl = templates.find((t) => t.provider_type === base.provider_type)
    setEditorMode('create')
    setEditorInitial(tpl
      ? { ...base, base_url: tpl.base_url, anthropic_base_url: tpl.anthropic_base_url ?? '' }
      : base)
    setEditingName(null)
    setEditorError(undefined)
    setEditorOpen(true)
  }

  function openEdit(ch: AdminChannel) {
    setEditorMode('edit')
    setEditorInitial(channelToForm(ch))
    setEditingName(ch.name)
    setEditorError(undefined)
    setEditorOpen(true)
  }

  function submitEditor(form: ChannelFormState) {
    setEditorError(undefined)
    const modelMap = rowsToModelMap(form.model_map)
    if (editorMode === 'create') {
      createMutation.mutate({
        name: form.name.trim(),
        provider_type: form.provider_type,
        base_url: form.base_url.trim(),
        api_key: form.api_key,
        anthropic_base_url: form.anthropic_base_url.trim() || null,
        model_map: Object.keys(modelMap).length ? modelMap : null,
        pricing: form.pricing || null,
      })
    } else if (editingName) {
      const body: UpdateChannelRequest = {}
      const original = channels.find((c) => c.name === editingName)
      if (!original) return
      if (form.provider_type !== original.provider_type) body.provider_type = form.provider_type
      if (form.base_url.trim() !== original.base_url) body.base_url = form.base_url.trim()
      const newAnthropic = form.anthropic_base_url.trim() || null
      if (newAnthropic !== (original.anthropic_base_url ?? null)) {
        body.anthropic_base_url = newAnthropic
      }
      if (!form.keep_existing_key && form.api_key) {
        body.api_key = form.api_key
      }
      // null clears the map server-side; omitting the field leaves it untouched.
      if (!sameModelMap(modelMap, original.model_map ?? {})) {
        body.model_map = Object.keys(modelMap).length ? modelMap : null
      }
      // '' from the dropdown means "untracked"; send it (empty clears on the server).
      if (form.pricing !== (original.pricing ?? '')) {
        body.pricing = form.pricing
      }
      if (Object.keys(body).length === 0) {
        setEditorOpen(false)
        push('No changes to save')
        return
      }
      updateMutation.mutate({ name: editingName, body })
    }
  }

  return (
    <>
      <Topbar
        breadcrumbs={[{ label: 'Configure' }, { label: 'Channels' }]}
        actions={
          <>
            <button className="btn btn-sm" onClick={() => void refetch()}>
              <Icon name="refresh" size={13} /> Refresh
            </button>
            <button className="btn btn-primary btn-sm" onClick={openCreate}>
              <Icon name="plus" size={13} /> New channel
            </button>
          </>
        }
      />
      <div className="page-pad">
        <div className="page-head">
          <h1 className="page-title">Channels</h1>
          <p className="page-sub">Upstream provider connections. Each channel maps to one provider account or endpoint.</p>
        </div>

        {isLoading && (
          <div style={{ display: 'flex', justifyContent: 'center', padding: 48 }}>
            <span className="spinner" style={{ width: 20, height: 20 }} />
          </div>
        )}

        {error && (
          <div style={{ padding: '12px 16px', background: 'var(--err-soft)', color: 'var(--err)', borderRadius: 'var(--r-md)', fontSize: 13 }}>
            Failed to load channels. {error instanceof Error ? error.message : ''}
          </div>
        )}

        {!isLoading && !error && (
          <>
            {/* Summary strip */}
            {channels.length > 0 && (
              <div style={{ display: 'flex', gap: 12, marginBottom: 16 }}>
                <div className="card" style={{ padding: '12px 20px', display: 'flex', alignItems: 'baseline', gap: 8 }}>
                  <span style={{ fontSize: 22, fontWeight: 600 }}>{channels.length}</span>
                  <span style={{ fontSize: 13, color: 'var(--muted)' }}>Total</span>
                </div>
                <div className="card" style={{ padding: '12px 20px', display: 'flex', alignItems: 'baseline', gap: 8 }}>
                  <span style={{ fontSize: 22, fontWeight: 600 }}>
                    {channels.filter((c) => c.anthropic_base_url).length}
                  </span>
                  <span style={{ fontSize: 13, color: 'var(--muted)' }}>Dual</span>
                </div>
              </div>
            )}

            <div className="card" style={{ overflow: 'hidden' }}>
              {channels.length === 0 ? (
                <Empty
                  icon="plug"
                  title="No channels configured"
                  sub="Click ‘New channel’ to connect an upstream LLM provider."
                />
              ) : (
                <div style={{ overflowX: 'auto' }}>
                <table className="table" style={{ tableLayout: 'fixed', width: '100%', minWidth: 940 }}>
                  <colgroup>
                    <col style={{ width: '21%' }} />
                    <col style={{ width: '11%' }} />
                    <col style={{ width: '13%' }} />
                    <col style={{ width: '28%' }} />
                    <col style={{ width: '17%' }} />
                    <col style={{ width: '10%' }} />
                  </colgroup>
                  <thead>
                    <tr>
                      <th>Channel</th>
                      <th>Provider</th>
                      <th>Billing</th>
                      <th>Endpoints</th>
                      <th>API Key</th>
                      <th style={{ textAlign: 'right' }}>Actions</th>
                    </tr>
                  </thead>
                  <tbody>
                    {channels.map((ch) => {
                      const masked = keyByName.get(ch.name)
                      return (
                        <tr key={ch.name} className="row-hover">
                          <td>
                            <div style={{ display: 'flex', alignItems: 'center', gap: 10, minWidth: 0 }}>
                              <ProviderMark kind={ch.provider_type} size={28} />
                              <span className="ellipsis" title={ch.name} style={{ fontWeight: 500 }}>
                                {ch.name}
                              </span>
                              {ch.anthropic_base_url && (
                                <span
                                  className="badge"
                                  style={{
                                    background: 'var(--info-soft)', color: 'var(--info)',
                                    borderColor: 'transparent', flexShrink: 0,
                                  }}
                                  title="Channel exposes both an OpenAI-compatible endpoint and an Anthropic endpoint"
                                >
                                  Dual
                                </span>
                              )}
                              {(() => {
                                const entries = Object.entries(ch.model_map ?? {})
                                if (entries.length === 0) return null
                                return (
                                  <span
                                    className="badge"
                                    style={{
                                      background: 'var(--surface-2)', color: 'var(--ink-2)',
                                      borderColor: 'transparent', flexShrink: 0,
                                    }}
                                    title={`Model mapping\n${entries.map(([k, v]) => `${k} → ${v}`).join('\n')}`}
                                  >
                                    {entries.length} mapped
                                  </span>
                                )
                              })()}
                            </div>
                          </td>
                          <td>
                            <span className="badge" title={ch.provider_type}>
                              <span className="ellipsis">{ch.provider_type}</span>
                            </span>
                          </td>
                          <td>
                            {(() => {
                              if (!ch.pricing) {
                                return <span className="muted" style={{ fontSize: 12 }}>—</span>
                              }
                              const rule = ruleByName.get(ch.pricing)
                              const isSub = rule?.type === 'subscription'
                              return (
                                <span
                                  className="badge"
                                  style={{
                                    background: isSub ? 'var(--brand-soft)' : 'var(--surface-2)',
                                    color: isSub ? 'var(--brand-ink)' : 'var(--ink-2)',
                                    borderColor: 'transparent',
                                  }}
                                  title={rule
                                    ? `${ch.pricing} — ${isSub ? `subscription $${rule.monthly_fee ?? 0}/mo` : 'pay-as-you-go'}`
                                    : `${ch.pricing} — pricing rule not found`}
                                >
                                  <span className="ellipsis">{ch.pricing}</span>
                                </span>
                              )
                            })()}
                          </td>
                          <td>
                            {ch.base_url || ch.anthropic_base_url ? (
                              <div style={{ display: 'grid', gap: 4, minWidth: 0 }}>
                                {ch.base_url && <EndpointLine label="OpenAI" url={ch.base_url} />}
                                {ch.anthropic_base_url && (
                                  <EndpointLine label="Anthropic" url={ch.anthropic_base_url} />
                                )}
                              </div>
                            ) : <span className="muted">—</span>}
                          </td>
                          <td>
                            {masked === undefined ? (
                              <span className="muted" style={{ fontSize: 12 }}>loading…</span>
                            ) : !masked ? (
                              <span className="muted">—</span>
                            ) : (
                              <div style={{ display: 'flex', alignItems: 'center', gap: 6, minWidth: 0 }}>
                                <code
                                  title={masked}
                                  style={{
                                    flex: '0 1 auto', minWidth: 0,
                                    fontFamily: 'var(--font-mono)', fontSize: 12, color: 'var(--muted)',
                                    overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap',
                                    background: 'var(--surface-2)', border: '1px solid var(--border)',
                                    borderRadius: 'var(--r-xs)', padding: '2px 8px',
                                  }}
                                >
                                  {masked}
                                </code>
                                <button
                                  className="btn btn-ghost btn-sm"
                                  style={{ padding: '0 6px', flexShrink: 0 }}
                                  onClick={() => copy(masked, 'Masked key copied')}
                                  title="Copy masked key"
                                >
                                  <Icon name="copy" size={12} />
                                </button>
                              </div>
                            )}
                          </td>
                          <td style={{ textAlign: 'right' }}>
                            <div style={{ display: 'inline-flex', gap: 4 }}>
                              <button
                                className="btn btn-ghost btn-sm"
                                title="Edit"
                                onClick={() => openEdit(ch)}
                                style={{ padding: '0 8px' }}
                              >
                                <Icon name="edit" size={13} />
                              </button>
                              <button
                                className="btn btn-ghost btn-sm"
                                title="Delete"
                                onClick={() => { setPendingDelete(ch); setDeleteError(undefined) }}
                                style={{ padding: '0 8px', color: 'var(--err)' }}
                              >
                                <Icon name="trash" size={13} />
                              </button>
                            </div>
                          </td>
                        </tr>
                      )
                    })}
                  </tbody>
                </table>
                </div>
              )}
            </div>
          </>
        )}
      </div>

      <ChannelEditor
        open={editorOpen}
        mode={editorMode}
        initial={editorInitial}
        templates={templates}
        rules={rules}
        busy={createMutation.isPending || updateMutation.isPending}
        error={editorError}
        onCancel={() => setEditorOpen(false)}
        onSubmit={submitEditor}
      />

      <Modal
        open={!!pendingDelete}
        onClose={() => { if (!deleteMutation.isPending) { setPendingDelete(null); setDeleteError(undefined) } }}
        title="Delete channel"
        width={460}
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
              disabled={deleteMutation.isPending}
              onClick={() => pendingDelete && deleteMutation.mutate(pendingDelete.name)}
            >
              {deleteMutation.isPending
                ? <span className="spinner" style={{ width: 12, height: 12 }} />
                : <Icon name="trash" size={13} />}
              Delete channel
            </button>
          </>
        }
      >
        {pendingDelete && (
          <div style={{ fontSize: 13, color: 'var(--ink-2)' }}>
            <p style={{ marginTop: 0 }}>
              You're about to delete <strong>{pendingDelete.name}</strong>.
            </p>
            <p>
              The gateway will refuse the delete if any router rule (or fallback) still points at this
              channel — fix the router first, then retry.
            </p>
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
