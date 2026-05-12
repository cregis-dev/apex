interface BreadcrumbItem {
  label: string
  href?: string
}

interface TopbarProps {
  breadcrumbs: BreadcrumbItem[]
  actions?: React.ReactNode
}

export default function Topbar({ breadcrumbs, actions }: TopbarProps) {
  return (
    <div style={{
      height: 56, flexShrink: 0,
      background: 'var(--surface)',
      borderBottom: '1px solid var(--border)',
      display: 'flex', alignItems: 'center',
      padding: '0 24px',
      position: 'sticky', top: 0, zIndex: 10,
    }}>
      {/* Breadcrumb */}
      <div style={{ display: 'flex', alignItems: 'center', gap: 6, flex: 1, fontSize: 13 }}>
        {breadcrumbs.map((crumb, i) => (
          <span key={i} style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
            {i > 0 && <span style={{ color: 'var(--muted-2)' }}>›</span>}
            <span style={{
              color: i === breadcrumbs.length - 1 ? 'var(--ink)' : 'var(--muted)',
              fontWeight: i === breadcrumbs.length - 1 ? 500 : 400,
            }}>
              {crumb.label}
            </span>
          </span>
        ))}
      </div>

      {actions && <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>{actions}</div>}

      {/* Avatar */}
      <div style={{
        width: 30, height: 30, borderRadius: '50%',
        background: 'oklch(0.75 0.02 60)',
        display: 'flex', alignItems: 'center', justifyContent: 'center',
        color: '#fff', fontSize: 11, fontWeight: 600,
        marginLeft: 16, flexShrink: 0,
      }}>
        AP
      </div>
    </div>
  )
}
