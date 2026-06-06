import { useEffect, useMemo, useState } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import Topbar from '../components/Topbar.tsx'
import Empty from '../components/Empty.tsx'
import Icon from '../components/Icon.tsx'
import Modal from '../components/Modal.tsx'
import RateLimitEditor from '../components/RateLimitEditor.tsx'
import StatusPill from '../components/StatusPill.tsx'
import { useToast } from '../components/Toast.tsx'
import { api } from '../lib/api.ts'
import type { AdminRouter, AdminTeam, CreateTeamRequest, RateLimit, UpdateTeamRequest } from '../lib/types.ts'

const DEFAULT_GROUP_LABEL = 'Default'

function fmt(n: number | null | undefined): string {
  if (n == null) return '—'
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`
  if (n >= 1_000) return `${(n / 1_000).toFixed(0)}k`
  return String(n)
}

function Monogram({ id }: { id: string }) {
  const letters = id.replace(/[^a-zA-Z0-9]/g, '').slice(0, 2).toUpperCase() || '··'
  return (
    <div style={{
      width: 28, height: 28, borderRadius: 6,
      background: 'oklch(0.75 0.06 55)',
      display: 'flex', alignItems: 'center', justifyContent: 'center',
      color: '#fff', fontSize: 11, fontWeight: 600, flexShrink: 0,
    }}>
      {letters}
    </div>
  )
}

function groupOf(team: AdminTeam): string {
  const g = team.group?.trim()
  return g && g.length > 0 ? g : DEFAULT_GROUP_LABEL
}

// ---------- Team form (used for both create and edit) ----------

interface TeamFormState {
  id: string
  group: string
  allowedRouters: string[]
  allowedModelsCsv: string
  rpm: string
  tpm: string
  enabled: boolean
}

function emptyForm(): TeamFormState {
  return {
    id: '',
    group: '',
    allowedRouters: [],
    allowedModelsCsv: '',
    rpm: '',
    tpm: '',
    enabled: true,
  }
}

function teamToForm(team: AdminTeam): TeamFormState {
  return {
    id: team.id,
    group: team.group ?? '',
    allowedRouters: team.policy.allowed_routers,
    allowedModelsCsv: (team.policy.allowed_models ?? []).join(', '),
    rpm: team.policy.rate_limit?.rpm != null ? String(team.policy.rate_limit.rpm) : '',
    tpm: team.policy.rate_limit?.tpm != null ? String(team.policy.rate_limit.tpm) : '',
    enabled: team.enabled,
  }
}

function parseInt32(value: string): number | null {
  const trimmed = value.trim()
  if (!trimmed) return null
  const n = Number(trimmed)
  if (!Number.isFinite(n) || n < 0) return null
  return Math.floor(n)
}

function buildCreatePayload(form: TeamFormState): CreateTeamRequest {
  const models = form.allowedModelsCsv
    .split(',')
    .map((m) => m.trim())
    .filter(Boolean)
  const rpm = parseInt32(form.rpm)
  const tpm = parseInt32(form.tpm)
  const hasLimit = rpm != null || tpm != null
  return {
    id: form.id.trim(),
    group: form.group.trim() || null,
    enabled: form.enabled,
    allowed_routers: form.allowedRouters,
    allowed_models: models.length > 0 ? models : null,
    rate_limit: hasLimit ? { rpm, tpm } : null,
  }
}

function buildUpdatePayload(form: TeamFormState, original: AdminTeam): UpdateTeamRequest {
  const payload: UpdateTeamRequest = {}
  const originalGroup = original.group ?? ''
  if (form.group.trim() !== originalGroup) {
    payload.group = form.group.trim() || null
  }
  if (form.enabled !== original.enabled) {
    payload.enabled = form.enabled
  }
  const sameRouters =
    form.allowedRouters.length === original.policy.allowed_routers.length &&
    form.allowedRouters.every((r) => original.policy.allowed_routers.includes(r))
  if (!sameRouters) {
    payload.allowed_routers = form.allowedRouters
  }
  const models = form.allowedModelsCsv.split(',').map((m) => m.trim()).filter(Boolean)
  const origModels = original.policy.allowed_models ?? []
  const sameModels =
    models.length === origModels.length && models.every((m) => origModels.includes(m))
  if (!sameModels) {
    payload.allowed_models = models.length > 0 ? models : null
  }
  const rpm = parseInt32(form.rpm)
  const tpm = parseInt32(form.tpm)
  const hasLimit = rpm != null || tpm != null
  const origRpm = original.policy.rate_limit?.rpm ?? null
  const origTpm = original.policy.rate_limit?.tpm ?? null
  if ((rpm ?? null) !== origRpm || (tpm ?? null) !== origTpm) {
    payload.rate_limit = hasLimit ? { rpm, tpm } : null
  }
  return payload
}

interface TeamEditorModalProps {
  open: boolean
  mode: 'create' | 'edit'
  initial: TeamFormState
  routers: AdminRouter[]
  existingGroups: string[]
  busy: boolean
  error?: string
  onCancel: () => void
  onSubmit: (form: TeamFormState) => void
}

function TeamEditorModal({
  open, mode, initial, routers, existingGroups,
  busy, error, onCancel, onSubmit,
}: TeamEditorModalProps) {
  const [form, setForm] = useState<TeamFormState>(initial)

  // Reset the form whenever the modal (re)opens with new initial values.
  useEffect(() => {
    if (open) setForm(initial)
  }, [open, initial])

  function toggleRouter(name: string) {
    setForm((f) => ({
      ...f,
      allowedRouters: f.allowedRouters.includes(name)
        ? f.allowedRouters.filter((r) => r !== name)
        : [...f.allowedRouters, name],
    }))
  }

  const idInvalid = mode === 'create' && !form.id.trim()
  const noRouters = form.allowedRouters.length === 0

  return (
    <Modal
      open={open}
      onClose={busy ? () => {} : onCancel}
      title={mode === 'create' ? 'Create team' : `Edit team · ${initial.id}`}
      width={560}
      footer={
        <>
          <button className="btn btn-sm" onClick={onCancel} disabled={busy}>Cancel</button>
          <button
            className="btn btn-primary btn-sm"
            disabled={busy || idInvalid || noRouters}
            onClick={() => onSubmit(form)}
          >
            {busy ? <span className="spinner" style={{ width: 12, height: 12 }} /> : null}
            {mode === 'create' ? 'Create team' : 'Save changes'}
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
        <Field label="Team ID">
          <input
            className="input"
            value={form.id}
            onChange={(e) => setForm((f) => ({ ...f, id: e.target.value }))}
            placeholder="e.g. growth-app"
            disabled={mode === 'edit'}
            style={{ width: '100%' }}
          />
          {mode === 'edit' && (
            <div style={{ fontSize: 11, color: 'var(--muted)', marginTop: 4 }}>
              Team ID cannot be changed after creation.
            </div>
          )}
        </Field>

        <Field label="Group">
          <input
            className="input"
            value={form.group}
            onChange={(e) => setForm((f) => ({ ...f, group: e.target.value }))}
            placeholder="e.g. engineering · data · research · qa"
            list="cp-existing-groups"
            style={{ width: '100%' }}
          />
          <datalist id="cp-existing-groups">
            {existingGroups.map((g) => <option key={g} value={g} />)}
          </datalist>
          <div style={{ fontSize: 11, color: 'var(--muted)', marginTop: 4 }}>
            Used to organize teams in the sidebar. Leave blank for "Default".
          </div>
        </Field>

        <Field label={`Allowed routers · ${form.allowedRouters.length}/${routers.length}`}>
          {routers.length === 0 ? (
            <div style={{ fontSize: 13, color: 'var(--muted)' }}>
              No routers are configured. Add routers in the gateway config first.
            </div>
          ) : (
            <div style={{ display: 'flex', flexWrap: 'wrap', gap: 6 }}>
              {routers.map((r) => {
                const active = form.allowedRouters.includes(r.name)
                return (
                  <button
                    key={r.name}
                    type="button"
                    onClick={() => toggleRouter(r.name)}
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
                    {r.name}
                  </button>
                )
              })}
            </div>
          )}
        </Field>

        <Field label="Allowed models (comma-separated, leave blank for all)">
          <input
            className="input"
            value={form.allowedModelsCsv}
            onChange={(e) => setForm((f) => ({ ...f, allowedModelsCsv: e.target.value }))}
            placeholder="gpt-4*, deepseek-*"
            style={{ width: '100%' }}
          />
        </Field>

        <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 12 }}>
          <Field label="Requests / minute (RPM)">
            <input
              className="input"
              value={form.rpm}
              onChange={(e) => setForm((f) => ({ ...f, rpm: e.target.value }))}
              placeholder="—"
              inputMode="numeric"
              style={{ width: '100%' }}
            />
          </Field>
          <Field label="Tokens / minute (TPM)">
            <input
              className="input"
              value={form.tpm}
              onChange={(e) => setForm((f) => ({ ...f, tpm: e.target.value }))}
              placeholder="—"
              inputMode="numeric"
              style={{ width: '100%' }}
            />
          </Field>
        </div>

        <Field label="Status">
          <label style={{ display: 'flex', alignItems: 'center', gap: 8, fontSize: 13, cursor: 'pointer' }}>
            <input
              type="checkbox"
              checked={form.enabled}
              onChange={(e) => setForm((f) => ({ ...f, enabled: e.target.checked }))}
            />
            {form.enabled
              ? <span style={{ color: 'var(--ok)' }}>Active — requests allowed</span>
              : <span style={{ color: 'var(--warn)' }}>Paused — requests rejected with 403</span>}
          </label>
        </Field>
      </div>
    </Modal>
  )
}

function Field({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div>
      <div style={{ fontSize: 12, fontWeight: 500, color: 'var(--ink-2)', marginBottom: 6 }}>
        {label}
      </div>
      {children}
    </div>
  )
}

// ---------- Page ----------

export default function TeamsPage() {
  const qc = useQueryClient()
  const { push } = useToast()
  const { data, isLoading, error, refetch } = useQuery({
    queryKey: ['teams'],
    queryFn: api.teams,
  })
  const { data: routersData } = useQuery({
    queryKey: ['routers'],
    queryFn: api.routers,
  })
  const { data: keysData, refetch: refetchKeys } = useQuery({
    queryKey: ['teams', 'api_keys'],
    queryFn: api.teamApiKeys,
    staleTime: 30_000,
  })

  const teams = data?.data ?? []
  const routers = routersData?.data ?? []
  const keyById = new Map((keysData?.data ?? []).map((e) => [e.id, e.api_key]))

  const [search, setSearch] = useState('')
  const [groupFilter, setGroupFilter] = useState<string>('')
  const [collapsed, setCollapsed] = useState<Set<string>>(new Set())

  const [editorOpen, setEditorOpen] = useState(false)
  const [editorMode, setEditorMode] = useState<'create' | 'edit'>('create')
  const [editorInitial, setEditorInitial] = useState<TeamFormState>(emptyForm())
  const [editorError, setEditorError] = useState<string | undefined>()
  const [editingTeam, setEditingTeam] = useState<AdminTeam | null>(null)
  const [createdKey, setCreatedKey] = useState<AdminTeam | null>(null)
  const [pendingDelete, setPendingDelete] = useState<AdminTeam | null>(null)
  const [deleteError, setDeleteError] = useState<string | undefined>()
  const [rlTeam, setRlTeam] = useState<AdminTeam | null>(null)
  const [rlError, setRlError] = useState<string | undefined>()

  const invalidate = () => {
    void qc.invalidateQueries({ queryKey: ['teams'] })
    void qc.invalidateQueries({ queryKey: ['teams', 'api_keys'] })
  }

  const createMutation = useMutation({
    mutationFn: (body: CreateTeamRequest) => api.createTeam(body),
    onSuccess: (created) => {
      invalidate()
      setEditorOpen(false)
      setCreatedKey(created)
      push(`Team "${created.id}" created`, 'ok')
    },
    onError: (err: unknown) => {
      setEditorError(err instanceof Error ? err.message : 'Failed to create team')
    },
  })

  const updateMutation = useMutation({
    mutationFn: ({ id, body }: { id: string; body: UpdateTeamRequest }) => api.updateTeam(id, body),
    onSuccess: (updated) => {
      invalidate()
      setEditorOpen(false)
      push(`Team "${updated.id}" updated`, 'ok')
    },
    onError: (err: unknown) => {
      setEditorError(err instanceof Error ? err.message : 'Failed to update team')
    },
  })

  const togglePauseMutation = useMutation({
    mutationFn: ({ id, enabled }: { id: string; enabled: boolean }) =>
      api.updateTeam(id, { enabled }),
    onSuccess: (updated) => {
      invalidate()
      push(`Team "${updated.id}" ${updated.enabled ? 'resumed' : 'paused'}`, 'ok')
    },
    onError: () => {
      push('Failed to change team status')
    },
  })

  const deleteMutation = useMutation({
    mutationFn: (id: string) => api.deleteTeam(id),
    onSuccess: (_, id) => {
      invalidate()
      setPendingDelete(null)
      setDeleteError(undefined)
      push(`Team "${id}" deleted`, 'ok')
    },
    onError: (err: unknown) => {
      setDeleteError(err instanceof Error ? err.message : 'Failed to delete team')
    },
  })

  const rateLimitMutation = useMutation({
    mutationFn: ({ id, rate_limit }: { id: string; rate_limit: RateLimit | null }) =>
      api.updateTeam(id, { rate_limit }),
    onSuccess: (updated) => {
      invalidate()
      const has = updated.policy.rate_limit?.rpm != null || updated.policy.rate_limit?.tpm != null
      push(`Rate limit ${has ? 'saved' : 'removed'} for "${updated.id}"`, 'ok')
      setRlTeam(null)
    },
    onError: (err: unknown) => {
      setRlError(err instanceof Error ? err.message : 'Failed to update rate limit')
    },
  })

  const allGroups = useMemo(() => {
    const set = new Set<string>()
    for (const t of teams) {
      const g = t.group?.trim()
      if (g) set.add(g)
    }
    return Array.from(set).sort()
  }, [teams])

  const filteredTeams = useMemo(() => {
    const q = search.trim().toLowerCase()
    return teams.filter((t) => {
      if (groupFilter && groupOf(t) !== groupFilter) return false
      if (!q) return true
      return (
        t.id.toLowerCase().includes(q) ||
        (t.group ?? '').toLowerCase().includes(q)
      )
    })
  }, [teams, search, groupFilter])

  const grouped = useMemo(() => {
    const m = new Map<string, AdminTeam[]>()
    for (const t of filteredTeams) {
      const g = groupOf(t)
      const arr = m.get(g) ?? []
      arr.push(t)
      m.set(g, arr)
    }
    const sorted = Array.from(m.entries()).sort(([a], [b]) => {
      // Default at the bottom; others alphabetical
      if (a === DEFAULT_GROUP_LABEL && b !== DEFAULT_GROUP_LABEL) return 1
      if (b === DEFAULT_GROUP_LABEL && a !== DEFAULT_GROUP_LABEL) return -1
      return a.localeCompare(b)
    })
    return sorted
  }, [filteredTeams])

  function openCreate() {
    setEditorMode('create')
    setEditorInitial(emptyForm())
    setEditingTeam(null)
    setEditorError(undefined)
    setEditorOpen(true)
  }

  function openEdit(team: AdminTeam) {
    setEditorMode('edit')
    setEditorInitial(teamToForm(team))
    setEditingTeam(team)
    setEditorError(undefined)
    setEditorOpen(true)
  }

  function submitEditor(form: TeamFormState) {
    setEditorError(undefined)
    if (editorMode === 'create') {
      createMutation.mutate(buildCreatePayload(form))
    } else if (editingTeam) {
      const body = buildUpdatePayload(form, editingTeam)
      if (Object.keys(body).length === 0) {
        setEditorOpen(false)
        push('No changes to save')
        return
      }
      updateMutation.mutate({ id: editingTeam.id, body })
    }
  }

  function toggleGroup(name: string) {
    setCollapsed((s) => {
      const next = new Set(s)
      next.has(name) ? next.delete(name) : next.add(name)
      return next
    })
  }

  function copy(text: string, label = 'Copied') {
    void navigator.clipboard.writeText(text).then(() => push(label, 'ok'))
  }

  // Fetch and copy a team's *full* api_key. The key is fetched async, so we hand
  // ClipboardItem a promise where available to keep the write inside the user
  // gesture (Safari is strict); otherwise fall back to fetch-then-writeText.
  async function copyFullKey(id: string) {
    try {
      if (typeof ClipboardItem !== 'undefined' && navigator.clipboard?.write) {
        await navigator.clipboard.write([
          new ClipboardItem({
            'text/plain': api
              .revealTeamApiKey(id)
              .then((r) => new Blob([r.api_key], { type: 'text/plain' })),
          }),
        ])
      } else {
        const r = await api.revealTeamApiKey(id)
        await navigator.clipboard.writeText(r.api_key)
      }
      push('API key copied', 'ok')
    } catch (err) {
      push(err instanceof Error ? `Failed to copy key: ${err.message}` : 'Failed to copy key')
    }
  }

  return (
    <>
      <Topbar
        breadcrumbs={[{ label: 'Access' }, { label: 'Teams' }]}
        actions={
          <div style={{ display: 'flex', gap: 8 }}>
            <button
              className="btn btn-sm"
              onClick={() => { void refetch(); void refetchKeys() }}
              title="Refresh"
            >
              <Icon name="refresh" size={13} />
              Refresh
            </button>
            <button className="btn btn-primary btn-sm" onClick={openCreate}>
              <Icon name="plus" size={13} />
              New team
            </button>
          </div>
        }
      />
      <div className="page-pad">
        <div className="page-head">
          <h1 className="page-title">Teams</h1>
          <p className="page-sub">
            Multi-tenant boundaries. Organize teams into groups — each team has its own key,
            quota, and model allowlist.
          </p>
        </div>

        {/* Summary strip */}
        {teams.length > 0 && (
          <div style={{ display: 'flex', gap: 12, marginBottom: 16, flexWrap: 'wrap' }}>
            <Stat label="Teams" value={teams.length} />
            <Stat label="Groups" value={Math.max(1, allGroups.length)} />
            <Stat label="Active" value={teams.filter((t) => t.enabled).length} />
            <Stat
              label="Paused"
              value={teams.filter((t) => !t.enabled).length}
              tone="warn"
            />
          </div>
        )}

        {/* Filter row */}
        {teams.length > 0 && (
          <div className="card" style={{ padding: '12px 16px', marginBottom: 16, display: 'flex', gap: 10, flexWrap: 'wrap', alignItems: 'center' }}>
            <label style={{ display: 'inline-flex', alignItems: 'center', gap: 6 }}>
              <span style={{ fontSize: 11, color: 'var(--muted)' }}>Group</span>
              <select
                className="select btn-sm"
                value={groupFilter}
                onChange={(e) => setGroupFilter(e.target.value)}
                style={{ height: 28, fontSize: 12, minWidth: 160 }}
              >
                <option value="">All groups</option>
                {allGroups.map((g) => <option key={g} value={g}>{g}</option>)}
                {teams.some((t) => groupOf(t) === DEFAULT_GROUP_LABEL) && (
                  <option value={DEFAULT_GROUP_LABEL}>{DEFAULT_GROUP_LABEL}</option>
                )}
              </select>
            </label>
            <div style={{ flex: 1, minWidth: 220, display: 'flex', alignItems: 'center', gap: 6 }}>
              <Icon name="search" size={13} style={{ color: 'var(--muted)' }} />
              <input
                className="input"
                placeholder="Search teams by id or group"
                value={search}
                onChange={(e) => setSearch(e.target.value)}
                style={{ flex: 1, height: 28, fontSize: 12 }}
              />
            </div>
            {(search || groupFilter) && (
              <button
                className="btn btn-sm"
                style={{ height: 28, fontSize: 12, color: 'var(--muted)' }}
                onClick={() => { setSearch(''); setGroupFilter('') }}
              >
                Clear
              </button>
            )}
          </div>
        )}

        {isLoading && (
          <div style={{ display: 'flex', justifyContent: 'center', padding: 48 }}>
            <span className="spinner" style={{ width: 20, height: 20 }} />
          </div>
        )}

        {error && (
          <div style={{ padding: '12px 16px', background: 'var(--err-soft)', color: 'var(--err)', borderRadius: 'var(--r-md)', fontSize: 13 }}>
            Failed to load teams.
          </div>
        )}

        {!isLoading && !error && teams.length === 0 && (
          <div className="card">
            <Empty
              icon="users"
              title="No teams configured"
              sub="Create your first team to enable multi-tenant access control."
            />
          </div>
        )}

        {!isLoading && !error && teams.length > 0 && filteredTeams.length === 0 && (
          <div className="card">
            <Empty icon="users" title="No teams match these filters" sub="Try clearing the search or group filter." />
          </div>
        )}

        {grouped.map(([group, rows]) => {
          const isCollapsed = collapsed.has(group)
          return (
            <div key={group} style={{ marginBottom: 16 }}>
              <div style={{
                display: 'flex', alignItems: 'center', gap: 10,
                padding: '8px 4px', cursor: 'pointer',
              }} onClick={() => toggleGroup(group)}>
                <Icon
                  name={isCollapsed ? 'chevron-right' : 'chevron-down'}
                  size={14}
                  style={{ color: 'var(--muted)' }}
                />
                <span style={{ fontWeight: 600, fontSize: 14 }}>{group}</span>
                <span className="badge">{rows.length}</span>
                <span style={{ flex: 1 }} />
                {group !== DEFAULT_GROUP_LABEL && (
                  <span style={{ fontSize: 11, color: 'var(--muted)' }}>
                    {rows.filter((t) => t.enabled).length} active · {rows.filter((t) => !t.enabled).length} paused
                  </span>
                )}
              </div>

              {!isCollapsed && (
                <div className="card">
                  <table className="table">
                    <thead>
                      <tr>
                        <th>Team</th>
                        <th>Status</th>
                        <th>API Key</th>
                        <th>Routers</th>
                        <th>Models</th>
                        <th>Rate limit</th>
                        <th style={{ textAlign: 'right', width: 140 }}>Actions</th>
                      </tr>
                    </thead>
                    <tbody>
                      {rows.map((team) => {
                        const rl = team.policy.rate_limit
                        const paused = !team.enabled
                        return (
                          <tr key={team.id} className="row-hover" style={{ opacity: paused ? 0.7 : 1 }}>
                            <td>
                              <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
                                <Monogram id={team.id} />
                                <div>
                                  <div style={{ fontWeight: 500 }}>{team.id}</div>
                                  <div style={{ fontSize: 11, color: 'var(--muted)' }}>{groupOf(team)}</div>
                                </div>
                              </div>
                            </td>
                            <td>
                              <StatusPill
                                status={paused ? 'warn' : 'ok'}
                                label={paused ? 'Paused' : 'Active'}
                              />
                            </td>
                            <td style={{ fontFamily: 'var(--font-mono)', fontSize: 12 }}>
                              {(() => {
                                const masked = keyById.get(team.id)
                                if (masked === undefined) {
                                  return <span className="muted" style={{ fontSize: 12 }}>loading…</span>
                                }
                                if (!masked) return <span className="muted">—</span>
                                return (
                                  <button
                                    className="btn btn-ghost btn-sm"
                                    onClick={() => copyFullKey(team.id)}
                                    style={{ padding: '2px 6px', fontFamily: 'var(--font-mono)' }}
                                    title="Copy full API key"
                                  >
                                    {masked}
                                    <Icon name="copy" size={11} />
                                  </button>
                                )
                              })()}
                            </td>
                            <td>
                              {team.policy.allowed_routers.length === 0 ? (
                                <span className="muted" style={{ fontSize: 12 }}>none</span>
                              ) : (
                                <div style={{ display: 'flex', gap: 4, flexWrap: 'wrap' }}>
                                  {team.policy.allowed_routers.slice(0, 3).map((r) => (
                                    <span key={r} className="badge">{r}</span>
                                  ))}
                                  {team.policy.allowed_routers.length > 3 && (
                                    <span className="badge">+{team.policy.allowed_routers.length - 3}</span>
                                  )}
                                </div>
                              )}
                            </td>
                            <td>
                              {(team.policy.allowed_models ?? []).length > 0 ? (
                                <div style={{ display: 'flex', gap: 4, flexWrap: 'wrap' }}>
                                  {(team.policy.allowed_models ?? []).slice(0, 2).map((m) => (
                                    <span key={m} className="badge mono" style={{ fontSize: 11 }}>{m}</span>
                                  ))}
                                  {(team.policy.allowed_models ?? []).length > 2 && (
                                    <span className="badge">+{(team.policy.allowed_models ?? []).length - 2}</span>
                                  )}
                                </div>
                              ) : <span className="muted" style={{ fontSize: 12 }}>all</span>}
                            </td>
                            <td>
                              <button
                                className="btn btn-ghost btn-sm"
                                title="Edit rate limit"
                                onClick={() => { setRlError(undefined); setRlTeam(team) }}
                                style={{ fontFamily: 'var(--font-mono)', fontSize: 12, padding: '2px 6px', gap: 6 }}
                              >
                                {rl?.rpm != null || rl?.tpm != null ? (
                                  <span>
                                    {rl?.rpm != null ? `${fmt(rl.rpm)} rpm` : ''}
                                    {rl?.rpm != null && rl?.tpm != null ? ' / ' : ''}
                                    {rl?.tpm != null ? `${fmt(rl.tpm)} tpm` : ''}
                                  </span>
                                ) : <span className="muted">no limit</span>}
                                <Icon name="edit" size={11} style={{ color: 'var(--muted-2)' }} />
                              </button>
                            </td>
                            <td style={{ textAlign: 'right' }}>
                              <div style={{ display: 'inline-flex', gap: 4 }}>
                                <button
                                  className="btn btn-ghost btn-sm"
                                  title={paused ? 'Resume' : 'Pause'}
                                  onClick={() =>
                                    togglePauseMutation.mutate({
                                      id: team.id,
                                      enabled: paused,
                                    })
                                  }
                                  disabled={togglePauseMutation.isPending}
                                  style={{ padding: '0 8px' }}
                                >
                                  <Icon name={paused ? 'play' : 'pause'} size={13} />
                                </button>
                                <button
                                  className="btn btn-ghost btn-sm"
                                  title="Edit"
                                  onClick={() => openEdit(team)}
                                  style={{ padding: '0 8px' }}
                                >
                                  <Icon name="edit" size={13} />
                                </button>
                                <button
                                  className="btn btn-ghost btn-sm"
                                  title="Delete"
                                  onClick={() => { setPendingDelete(team); setDeleteError(undefined) }}
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
          )
        })}
      </div>

      <TeamEditorModal
        open={editorOpen}
        mode={editorMode}
        initial={editorInitial}
        routers={routers}
        existingGroups={allGroups}
        busy={createMutation.isPending || updateMutation.isPending}
        error={editorError}
        onCancel={() => setEditorOpen(false)}
        onSubmit={submitEditor}
      />

      {/* Reveal newly-created api key */}
      <Modal
        open={!!createdKey}
        onClose={() => setCreatedKey(null)}
        title="API key — save it now"
        width={520}
        footer={
          <button className="btn btn-primary btn-sm" onClick={() => setCreatedKey(null)}>
            Done
          </button>
        }
      >
        {createdKey && (
          <div>
            <p style={{ margin: '0 0 14px', fontSize: 13, color: 'var(--ink-2)' }}>
              This is the only time the full key for <strong>{createdKey.id}</strong> will be shown.
              Copy it now and hand it to the team owner.
            </p>
            <div style={{
              display: 'flex', alignItems: 'center', gap: 8,
              padding: '12px 14px', borderRadius: 'var(--r-sm)',
              background: 'var(--surface-2)', border: '1px solid var(--border)',
            }}>
              <code style={{
                fontFamily: 'var(--font-mono)', fontSize: 12,
                flex: 1, wordBreak: 'break-all',
              }}>
                {createdKey.api_key ?? '(server did not return the key)'}
              </code>
              <button
                className="btn btn-sm"
                disabled={!createdKey.api_key}
                onClick={() => createdKey.api_key && copy(createdKey.api_key, 'API key copied')}
              >
                <Icon name="copy" size={13} /> Copy
              </button>
            </div>
            <div style={{
              marginTop: 14, padding: '8px 12px',
              background: 'var(--info-soft)', color: 'var(--info)',
              borderRadius: 'var(--r-sm)', fontSize: 12,
            }}>
              On subsequent loads this key will appear masked. To rotate, delete and re-create the team.
            </div>
          </div>
        )}
      </Modal>

      {/* Delete confirmation */}
      <Modal
        open={!!pendingDelete}
        onClose={() => { if (!deleteMutation.isPending) { setPendingDelete(null); setDeleteError(undefined) } }}
        title="Delete team"
        width={440}
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
              onClick={() => pendingDelete && deleteMutation.mutate(pendingDelete.id)}
            >
              {deleteMutation.isPending
                ? <span className="spinner" style={{ width: 12, height: 12 }} />
                : <Icon name="trash" size={13} />}
              Delete team
            </button>
          </>
        }
      >
        {pendingDelete && (
          <div style={{ fontSize: 13, color: 'var(--ink-2)' }}>
            <p style={{ marginTop: 0 }}>
              You're about to delete <strong>{pendingDelete.id}</strong>
              {pendingDelete.group ? <> from <strong>{pendingDelete.group}</strong></> : null}.
            </p>
            <p>
              Any client still using its API key will start receiving <code>401 Invalid Team API Key</code>.
              If you only want to temporarily block it, use <strong>Pause</strong> instead.
            </p>
            {deleteError && (
              <div style={{ marginTop: 10, padding: '8px 12px', borderRadius: 'var(--r-sm)', background: 'var(--err-soft)', color: 'var(--err)', fontSize: 12 }}>
                {deleteError}
              </div>
            )}
          </div>
        )}
      </Modal>

      {rlTeam && (
        <RateLimitEditor
          key={rlTeam.id}
          team={rlTeam}
          busy={rateLimitMutation.isPending}
          error={rlError}
          onCancel={() => setRlTeam(null)}
          onSubmit={(rate_limit) => { setRlError(undefined); rateLimitMutation.mutate({ id: rlTeam.id, rate_limit }) }}
        />
      )}
    </>
  )
}

function Stat({ label, value, tone }: { label: string; value: number; tone?: 'warn' }) {
  const color = tone === 'warn' && value > 0 ? 'var(--warn)' : 'var(--ink)'
  return (
    <div className="card" style={{ padding: '12px 20px', display: 'flex', alignItems: 'baseline', gap: 8 }}>
      <span style={{ fontSize: 22, fontWeight: 600, color }}>{value}</span>
      <span style={{ fontSize: 13, color: 'var(--muted)' }}>{label}</span>
    </div>
  )
}
