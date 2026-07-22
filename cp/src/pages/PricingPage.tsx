import { useEffect, useState } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import Topbar from '../components/Topbar.tsx'
import Icon from '../components/Icon.tsx'
import Modal from '../components/Modal.tsx'
import Empty from '../components/Empty.tsx'
import { useToast } from '../components/Toast.tsx'
import { api } from '../lib/api.ts'
import type { ModelPriceRow, PricingConfig, PricingRule } from '../lib/types.ts'

// ---------- form model ----------

interface PriceRowForm { match: string; input: string; output: string; cache_read: string; cache_write: string }
interface RuleForm {
  name: string
  type: 'payg' | 'subscription'
  prices: PriceRowForm[]
  monthly_fee: string
  billing_day: string
  quota: string
}

const num = (s: string) => (s.trim() ? Number(s) : 0)
const optNum = (s: string) => (s.trim() ? Number(s) : null)

function emptyPriceRow(match = ''): PriceRowForm {
  return { match, input: '', output: '', cache_read: '', cache_write: '' }
}

function emptyForm(): RuleForm {
  return { name: '', type: 'payg', prices: [emptyPriceRow('*')], monthly_fee: '', billing_day: '1', quota: '' }
}

function ruleToForm(r: PricingRule): RuleForm {
  const rows = (r.prices ?? []).map((p) => ({
    match: p.match,
    input: p.input != null ? String(p.input) : '',
    output: p.output != null ? String(p.output) : '',
    cache_read: p.cache_read != null ? String(p.cache_read) : '',
    cache_write: p.cache_write != null ? String(p.cache_write) : '',
  }))
  return {
    name: r.name,
    type: r.type === 'subscription' ? 'subscription' : 'payg',
    prices: rows.length ? rows : [emptyPriceRow('*')],
    monthly_fee: r.monthly_fee != null ? String(r.monthly_fee) : '',
    billing_day: r.billing_day != null ? String(r.billing_day) : '1',
    quota: r.included_quota_tokens != null ? String(r.included_quota_tokens) : '',
  }
}

function formToRule(f: RuleForm): PricingRule {
  if (f.type === 'subscription') {
    return {
      name: f.name.trim(),
      type: 'subscription',
      monthly_fee: num(f.monthly_fee),
      billing_day: Number(f.billing_day) || 1,
      included_quota_tokens: optNum(f.quota),
    }
  }
  const prices: ModelPriceRow[] = f.prices
    .filter((p) => p.match.trim())
    .map((p) => ({
      match: p.match.trim(),
      input: num(p.input),
      output: num(p.output),
      cache_read: optNum(p.cache_read),
      cache_write: optNum(p.cache_write),
    }))
  return { name: f.name.trim(), type: 'payg', prices }
}

// ---------- editor modal ----------

function Field({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div>
      <div style={{ fontSize: 12, fontWeight: 500, color: 'var(--ink-2)', marginBottom: 6 }}>{label}</div>
      {children}
    </div>
  )
}

function NumInput({ value, onChange, placeholder }: { value: string; onChange: (v: string) => void; placeholder?: string }) {
  return (
    <input
      className="input btn-sm"
      value={value}
      onChange={(e) => onChange(e.target.value)}
      placeholder={placeholder ?? '0'}
      inputMode="decimal"
      style={{ width: '100%', height: 30, textAlign: 'right', fontFamily: 'var(--font-mono)', fontSize: 12 }}
    />
  )
}

function RuleEditor({
  open, mode, initial, existingNames, busy, error, onCancel, onSubmit,
}: {
  open: boolean
  mode: 'create' | 'edit'
  initial: RuleForm
  existingNames: string[]
  busy: boolean
  error?: string
  onCancel: () => void
  onSubmit: (f: RuleForm) => void
}) {
  const [form, setForm] = useState<RuleForm>(initial)
  useEffect(() => { if (open) setForm(initial) }, [open, initial])

  const set = <K extends keyof RuleForm>(k: K, v: RuleForm[K]) => setForm((f) => ({ ...f, [k]: v }))
  const setRow = (i: number, k: keyof PriceRowForm, v: string) =>
    setForm((f) => ({ ...f, prices: f.prices.map((p, idx) => (idx === i ? { ...p, [k]: v } : p)) }))
  const addRow = () => setForm((f) => ({ ...f, prices: [...f.prices, emptyPriceRow()] }))
  const removeRow = (i: number) => setForm((f) => ({ ...f, prices: f.prices.filter((_, idx) => idx !== i) }))

  const name = form.name.trim()
  const dupName = mode === 'create' && existingNames.includes(name)
  const invalid = !name || dupName
  const isSub = form.type === 'subscription'

  return (
    <Modal
      open={open}
      onClose={busy ? () => {} : onCancel}
      title={mode === 'create' ? 'New pricing rule' : `Edit rule · ${initial.name}`}
      width={640}
      footer={
        <>
          <button className="btn btn-sm" onClick={onCancel} disabled={busy}>Cancel</button>
          <button className="btn btn-primary btn-sm" disabled={busy || invalid} onClick={() => onSubmit(form)}>
            {busy ? <span className="spinner" style={{ width: 12, height: 12 }} /> : null}
            {mode === 'create' ? 'Create rule' : 'Save changes'}
          </button>
        </>
      }
    >
      {error && (
        <div style={{ padding: '8px 12px', marginBottom: 14, borderRadius: 'var(--r-sm)', background: 'var(--err-soft)', color: 'var(--err)', fontSize: 13 }}>{error}</div>
      )}

      <div style={{ display: 'grid', gap: 14 }}>
        <div style={{ display: 'grid', gridTemplateColumns: '1fr 200px', gap: 12 }}>
          <Field label="Rule name">
            <input
              className="input"
              value={form.name}
              onChange={(e) => set('name', e.target.value)}
              placeholder="e.g. deepseek or claude-max"
              style={{ width: '100%', fontFamily: 'var(--font-mono)' }}
            />
            {dupName && <div style={{ fontSize: 11, color: 'var(--err)', marginTop: 4 }}>A rule with this name already exists.</div>}
          </Field>
          <Field label="Type">
            <select className="select" value={form.type} onChange={(e) => set('type', e.target.value as RuleForm['type'])} style={{ width: '100%' }}>
              <option value="payg">Pay-as-you-go</option>
              <option value="subscription">Subscription</option>
            </select>
          </Field>
        </div>

        {isSub ? (
          <>
            <div style={{ fontSize: 11, fontWeight: 600, color: 'var(--muted)', textTransform: 'uppercase', letterSpacing: '0.04em' }}>Subscription</div>
            <div style={{ display: 'grid', gridTemplateColumns: 'repeat(3, 1fr)', gap: 12 }}>
              <Field label="Monthly fee"><NumInput value={form.monthly_fee} onChange={(v) => set('monthly_fee', v)} placeholder="20" /></Field>
              <Field label="Renews on day"><NumInput value={form.billing_day} onChange={(v) => set('billing_day', v)} placeholder="1" /></Field>
              <Field label="Included quota tokens"><NumInput value={form.quota} onChange={(v) => set('quota', v)} placeholder="optional" /></Field>
            </div>
          </>
        ) : (
          <>
            <div style={{ fontSize: 11, fontWeight: 600, color: 'var(--muted)', textTransform: 'uppercase', letterSpacing: '0.04em' }}>
              Rate card · per-model rows (first match wins — keep <code>*</code> last)
            </div>
            {/* header */}
            <div style={{ display: 'grid', gridTemplateColumns: '1.4fr 1fr 1fr 1fr 1fr 28px', gap: 8, fontSize: 10, color: 'var(--muted)', textTransform: 'uppercase', letterSpacing: '0.04em', padding: '0 2px' }}>
              <span>Model match</span><span style={{ textAlign: 'right' }}>Input</span><span style={{ textAlign: 'right' }}>Output</span><span style={{ textAlign: 'right' }}>Cache read</span><span style={{ textAlign: 'right' }}>Cache write</span><span />
            </div>
            {form.prices.map((row, i) => (
              <div key={i} style={{ display: 'grid', gridTemplateColumns: '1.4fr 1fr 1fr 1fr 1fr 28px', gap: 8, alignItems: 'center' }}>
                <input
                  className="input btn-sm"
                  value={row.match}
                  onChange={(e) => setRow(i, 'match', e.target.value)}
                  placeholder="* or *pro*"
                  style={{ height: 30, fontFamily: 'var(--font-mono)', fontSize: 12 }}
                />
                <NumInput value={row.input} onChange={(v) => setRow(i, 'input', v)} />
                <NumInput value={row.output} onChange={(v) => setRow(i, 'output', v)} />
                <NumInput value={row.cache_read} onChange={(v) => setRow(i, 'cache_read', v)} placeholder="—" />
                <NumInput value={row.cache_write} onChange={(v) => setRow(i, 'cache_write', v)} placeholder="—" />
                <button
                  className="btn btn-ghost btn-sm"
                  title="Remove row"
                  onClick={() => removeRow(i)}
                  disabled={form.prices.length <= 1}
                  style={{ padding: 0, height: 30, color: 'var(--err)' }}
                >
                  <Icon name="trash" size={12} />
                </button>
              </div>
            ))}
            <button className="btn btn-sm" onClick={addRow} style={{ alignSelf: 'flex-start' }}>
              <Icon name="plus" size={12} /> Add model price
            </button>
          </>
        )}
      </div>
    </Modal>
  )
}

// ---------- read-only summaries ----------

function feeOrRates(r: PricingRule, currency: string): string {
  if (r.type === 'subscription') return `${currency} ${r.monthly_fee ?? 0}/mo`
  const n = (r.prices ?? []).length
  return `${n} model${n === 1 ? '' : 's'}`
}

function details(r: PricingRule): string {
  if (r.type === 'subscription') {
    const q = r.included_quota_tokens != null ? `${(r.included_quota_tokens / 1_000_000).toFixed(1)}M quota` : 'no quota'
    return `renews day ${r.billing_day ?? 1} · ${q}`
  }
  return (r.prices ?? []).map((p) => p.match).join(' · ') || '(no rows)'
}

// ---------- page ----------

export default function PricingPage() {
  const qc = useQueryClient()
  const { push } = useToast()
  const { data, isLoading, error } = useQuery({ queryKey: ['pricing'], queryFn: api.pricing })

  const [currency, setCurrency] = useState('USD')
  const [unit, setUnit] = useState('1000000')
  useEffect(() => {
    if (data) { setCurrency(data.currency || 'USD'); setUnit(String(data.unit || 1_000_000)) }
  }, [data])

  const rules = data?.rules ?? []

  const [editorOpen, setEditorOpen] = useState(false)
  const [editorMode, setEditorMode] = useState<'create' | 'edit'>('create')
  const [editorInitial, setEditorInitial] = useState<RuleForm>(emptyForm())
  const [editingName, setEditingName] = useState<string | null>(null)
  const [editorError, setEditorError] = useState<string | undefined>()
  const [pendingDelete, setPendingDelete] = useState<string | null>(null)

  const save = useMutation({
    mutationFn: (body: PricingConfig) => api.savePricing(body),
    onSuccess: (saved) => {
      qc.setQueryData(['pricing'], saved)
      void qc.invalidateQueries({ queryKey: ['analytics'] })
      void qc.invalidateQueries({ queryKey: ['channels'] })
      setEditorOpen(false); setPendingDelete(null)
      push('Pricing saved — applied live', 'ok')
    },
    onError: (err: unknown) => {
      setEditorError(err instanceof Error ? err.message : 'Save failed')
      push(err instanceof Error ? `Save failed: ${err.message}` : 'Save failed')
    },
  })

  const meta = () => ({ currency: currency.trim() || 'USD', unit: Number(unit) || 1_000_000 })
  const metaDirty = !!data && (currency !== data.currency || Number(unit) !== data.unit)
  const persist = (nextRules: PricingRule[]) => save.mutate({ ...meta(), rules: nextRules })

  function openCreate() { setEditorMode('create'); setEditorInitial(emptyForm()); setEditingName(null); setEditorError(undefined); setEditorOpen(true) }
  function openEdit(r: PricingRule) { setEditorMode('edit'); setEditorInitial(ruleToForm(r)); setEditingName(r.name); setEditorError(undefined); setEditorOpen(true) }
  function submitEditor(f: RuleForm) {
    setEditorError(undefined)
    const rule = formToRule(f)
    const next = editorMode === 'edit' && editingName
      ? rules.map((r) => (r.name === editingName ? rule : r))
      : [...rules, rule]
    persist(next)
  }

  return (
    <>
      <Topbar
        breadcrumbs={[{ label: 'Configure' }, { label: 'Pricing' }]}
        actions={<button className="btn btn-primary btn-sm" onClick={openCreate}><Icon name="plus" size={13} /> New rule</button>}
      />
      <div className="page-pad" style={{ maxWidth: 960 }}>
        <div className="page-head">
          <h1 className="page-title">Pricing</h1>
          <p className="page-sub">
            Named billing rules for cost tracking. Each channel selects one rule (on the Channels page).
            A pay-as-you-go rule is a rate card — price each model differently within the same channel.
          </p>
        </div>

        {isLoading && <div style={{ display: 'flex', justifyContent: 'center', padding: 48 }}><span className="spinner" style={{ width: 20, height: 20 }} /></div>}
        {error && <div style={{ padding: '12px 16px', background: 'var(--err-soft)', color: 'var(--err)', borderRadius: 'var(--r-md)', fontSize: 13 }}>Failed to load pricing. {error instanceof Error ? error.message : ''}</div>}

        {!isLoading && !error && (
          <>
            <div className="card" style={{ padding: '12px 16px', marginBottom: 16, display: 'flex', gap: 20, flexWrap: 'wrap', alignItems: 'center' }}>
              <label style={{ display: 'inline-flex', alignItems: 'center', gap: 8 }}>
                <span style={{ fontSize: 12, color: 'var(--muted)' }}>Currency</span>
                <input className="input btn-sm" value={currency} onChange={(e) => setCurrency(e.target.value)} style={{ width: 80, height: 30 }} />
              </label>
              <label style={{ display: 'inline-flex', alignItems: 'center', gap: 8 }}>
                <span style={{ fontSize: 12, color: 'var(--muted)' }}>Rates per</span>
                <input className="input btn-sm" value={unit} onChange={(e) => setUnit(e.target.value)} inputMode="numeric" style={{ width: 120, height: 30, fontFamily: 'var(--font-mono)' }} />
                <span style={{ fontSize: 12, color: 'var(--muted)' }}>tokens</span>
              </label>
              {metaDirty && <button className="btn btn-sm btn-primary" disabled={save.isPending} onClick={() => persist(rules)} style={{ height: 30 }}>Save settings</button>}
            </div>

            <div className="card">
              {rules.length === 0 ? (
                <Empty icon="book" title="No pricing rules yet" sub="Click ‘New rule’ to add one, then assign it to channels." />
              ) : (
                <table className="table" style={{ tableLayout: 'fixed', width: '100%' }}>
                  <colgroup>
                    <col style={{ width: '24%' }} /><col style={{ width: '15%' }} /><col style={{ width: '15%' }} /><col style={{ width: '34%' }} /><col style={{ width: '12%' }} />
                  </colgroup>
                  <thead>
                    <tr><th>Rule</th><th>Type</th><th style={{ textAlign: 'right' }}>Fee / rates</th><th>Details</th><th style={{ textAlign: 'right' }}>Actions</th></tr>
                  </thead>
                  <tbody>
                    {rules.map((r) => {
                      const isSub = r.type === 'subscription'
                      return (
                        <tr key={r.name} className="row-hover">
                          <td style={{ fontWeight: 500, fontFamily: 'var(--font-mono)', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>{r.name}</td>
                          <td><span className="badge" style={isSub ? { background: 'var(--brand-soft)', color: 'var(--brand-ink)', borderColor: 'transparent' } : undefined}>{isSub ? 'Subscription' : 'PAYG'}</span></td>
                          <td style={{ textAlign: 'right', fontFamily: 'var(--font-mono)', fontSize: 12 }}>{feeOrRates(r, currency)}</td>
                          <td style={{ fontSize: 12, color: 'var(--muted)', fontFamily: 'var(--font-mono)', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }} title={details(r)}>{details(r)}</td>
                          <td style={{ textAlign: 'right' }}>
                            <div style={{ display: 'inline-flex', gap: 4 }}>
                              <button className="btn btn-ghost btn-sm" title="Edit" onClick={() => openEdit(r)} style={{ padding: '0 8px' }}><Icon name="edit" size={13} /></button>
                              <button className="btn btn-ghost btn-sm" title="Delete" onClick={() => setPendingDelete(r.name)} style={{ padding: '0 8px', color: 'var(--err)' }}><Icon name="trash" size={13} /></button>
                            </div>
                          </td>
                        </tr>
                      )
                    })}
                  </tbody>
                </table>
              )}
            </div>

            <p style={{ fontSize: 12, color: 'var(--muted)', marginTop: 12 }}>
              Rates are per {unit || '1000000'} tokens (cache miss = input, cache hit = cache read). Cache write defaults to the input rate when blank.
              Assign a rule to a channel on the Channels page.
            </p>
          </>
        )}
      </div>

      <RuleEditor
        open={editorOpen}
        mode={editorMode}
        initial={editorInitial}
        existingNames={rules.map((r) => r.name)}
        busy={save.isPending}
        error={editorError}
        onCancel={() => setEditorOpen(false)}
        onSubmit={submitEditor}
      />

      <Modal
        open={!!pendingDelete}
        onClose={() => { if (!save.isPending) setPendingDelete(null) }}
        title="Delete pricing rule"
        width={440}
        footer={
          <>
            <button className="btn btn-sm" onClick={() => setPendingDelete(null)} disabled={save.isPending}>Cancel</button>
            <button className="btn btn-sm" style={{ background: 'var(--err)', color: '#fff', borderColor: 'transparent' }} disabled={save.isPending} onClick={() => pendingDelete && persist(rules.filter((r) => r.name !== pendingDelete))}>
              {save.isPending ? <span className="spinner" style={{ width: 12, height: 12 }} /> : <Icon name="trash" size={13} />}
              Delete rule
            </button>
          </>
        }
      >
        {pendingDelete && (
          <p style={{ fontSize: 13, color: 'var(--ink-2)', marginTop: 0 }}>
            Delete rule <strong>{pendingDelete}</strong>? Channels still pointing at it will become untracked (no cost) until you assign another rule.
          </p>
        )}
      </Modal>
    </>
  )
}
