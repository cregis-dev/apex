import type { TimeRange, FilterOptions } from '../lib/types.ts'

const RANGE_OPTIONS: { label: string; value: TimeRange }[] = [
  { label: 'Last 1h', value: '1h' },
  { label: 'Last 24h', value: '24h' },
  { label: 'Last 7d', value: '7d' },
  { label: 'Last 30d', value: '30d' },
]

const STATUS_OPTIONS: { label: string; value: string }[] = [
  { label: 'All statuses', value: '' },
  { label: 'Success', value: 'success' },
  { label: 'Error', value: 'error' },
  { label: 'Fallback success', value: 'fallback_success' },
  { label: 'Fallback error', value: 'fallback_error' },
]

export interface FilterValues {
  range: TimeRange
  team_id: string
  router: string
  channel: string
  model: string
  status: string
}

export const DEFAULT_FILTERS: FilterValues = {
  range: '24h',
  team_id: '',
  router: '',
  channel: '',
  model: '',
  status: '',
}

export interface FiltersBarProps {
  values: FilterValues
  options: FilterOptions | undefined
  onChange: (next: FilterValues) => void
  showStatus?: boolean
  showRange?: boolean
}

function Select({
  label,
  value,
  options,
  placeholder,
  onChange,
}: {
  label: string
  value: string
  options: string[] | { label: string; value: string }[]
  placeholder: string
  onChange: (v: string) => void
}) {
  const normalized = options.map((o) =>
    typeof o === 'string' ? { label: o, value: o } : o
  )
  return (
    <label style={{ display: 'inline-flex', alignItems: 'center', gap: 6 }}>
      <span style={{ fontSize: 11, color: 'var(--muted)', whiteSpace: 'nowrap' }}>{label}</span>
      <select
        className="select btn-sm"
        value={value}
        onChange={(e) => onChange(e.target.value)}
        style={{ height: 28, fontSize: 12, minWidth: 120, maxWidth: 200 }}
      >
        <option value="">{placeholder}</option>
        {normalized.map((o) => (
          <option key={o.value} value={o.value}>{o.label}</option>
        ))}
      </select>
    </label>
  )
}

export default function FiltersBar({
  values,
  options,
  onChange,
  showStatus = false,
  showRange = true,
}: FiltersBarProps) {
  const isFiltered =
    values.team_id || values.router || values.channel || values.model || values.status

  const setOne = <K extends keyof FilterValues>(key: K, v: FilterValues[K]) => {
    onChange({ ...values, [key]: v })
  }

  return (
    <div style={{ display: 'flex', alignItems: 'center', gap: 10, flexWrap: 'wrap' }}>
      {showRange && (
        <label style={{ display: 'inline-flex', alignItems: 'center', gap: 6 }}>
          <span style={{ fontSize: 11, color: 'var(--muted)' }}>Range</span>
          <select
            className="select btn-sm"
            value={values.range}
            onChange={(e) => setOne('range', e.target.value as TimeRange)}
            style={{ height: 28, fontSize: 12 }}
          >
            {RANGE_OPTIONS.map((r) => (
              <option key={r.value} value={r.value}>{r.label}</option>
            ))}
          </select>
        </label>
      )}

      <Select
        label="Team"
        value={values.team_id}
        options={options?.teams ?? []}
        placeholder="All teams"
        onChange={(v) => setOne('team_id', v)}
      />
      <Select
        label="Router"
        value={values.router}
        options={options?.routers ?? []}
        placeholder="All routers"
        onChange={(v) => setOne('router', v)}
      />
      <Select
        label="Channel"
        value={values.channel}
        options={options?.channels ?? []}
        placeholder="All channels"
        onChange={(v) => setOne('channel', v)}
      />
      <Select
        label="Model"
        value={values.model}
        options={options?.models ?? []}
        placeholder="All models"
        onChange={(v) => setOne('model', v)}
      />
      {showStatus && (
        <Select
          label="Status"
          value={values.status}
          options={STATUS_OPTIONS.filter((o) => o.value !== '')}
          placeholder="All statuses"
          onChange={(v) => setOne('status', v)}
        />
      )}

      {isFiltered && (
        <button
          className="btn btn-sm"
          style={{ height: 28, fontSize: 12, color: 'var(--muted)' }}
          onClick={() => onChange({ ...DEFAULT_FILTERS, range: values.range })}
          title="Clear filters"
        >
          Clear
        </button>
      )}
    </div>
  )
}

export function filterValuesToParams(v: FilterValues): Record<string, string> {
  const out: Record<string, string> = { range: v.range }
  if (v.team_id) out.team_id = v.team_id
  if (v.router) out.router = v.router
  if (v.channel) out.channel = v.channel
  if (v.model) out.model = v.model
  if (v.status) out.status = v.status
  return out
}
