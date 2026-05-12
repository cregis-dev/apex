import Icon, { type IconName } from './Icon.tsx'

interface EmptyProps {
  icon?: IconName
  title: string
  sub?: string
  action?: React.ReactNode
}

export default function Empty({ icon = 'info', title, sub, action }: EmptyProps) {
  return (
    <div style={{
      display: 'flex', flexDirection: 'column', alignItems: 'center',
      justifyContent: 'center', padding: '48px 24px', gap: 12,
      color: 'var(--muted)',
    }}>
      <div style={{
        width: 40, height: 40, borderRadius: 'var(--r-md)',
        background: 'var(--surface-2)', border: '1px solid var(--border)',
        display: 'flex', alignItems: 'center', justifyContent: 'center',
      }}>
        <Icon name={icon} size={18} />
      </div>
      <div style={{ textAlign: 'center' }}>
        <div style={{ fontWeight: 500, color: 'var(--ink-2)', marginBottom: 4 }}>{title}</div>
        {sub && <div style={{ fontSize: 13, color: 'var(--muted)' }}>{sub}</div>}
      </div>
      {action}
    </div>
  )
}
