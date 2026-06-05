import { useEffect, useRef, useState } from 'react'
import { useQuery } from '@tanstack/react-query'
import { api } from '../lib/api.ts'
import { clearToken } from '../lib/auth.ts'
import Icon from './Icon.tsx'

interface BreadcrumbItem {
  label: string
  href?: string
}

interface TopbarProps {
  breadcrumbs: BreadcrumbItem[]
  actions?: React.ReactNode
}

export default function Topbar({ breadcrumbs, actions }: TopbarProps) {
  const [menuOpen, setMenuOpen] = useState(false)
  const wrapRef = useRef<HTMLDivElement>(null)
  const { data: info } = useQuery({
    queryKey: ['cpInfo'],
    queryFn: api.cpInfo,
    staleTime: 60_000,
  })

  useEffect(() => {
    if (!menuOpen) return
    function handler(e: MouseEvent) {
      if (!wrapRef.current?.contains(e.target as Node)) setMenuOpen(false)
    }
    window.addEventListener('mousedown', handler)
    return () => window.removeEventListener('mousedown', handler)
  }, [menuOpen])

  function signOut() {
    clearToken()
    window.location.reload()
  }

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

      {/* Avatar / account menu */}
      <div ref={wrapRef} style={{ position: 'relative', marginLeft: 16 }}>
        <button
          type="button"
          onClick={() => setMenuOpen((o) => !o)}
          aria-label="Account menu"
          aria-expanded={menuOpen}
          style={{
            width: 30, height: 30, borderRadius: '50%',
            background: 'oklch(0.75 0.02 60)',
            display: 'flex', alignItems: 'center', justifyContent: 'center',
            color: '#fff', fontSize: 11, fontWeight: 600,
            border: 'none', cursor: 'pointer', padding: 0,
          }}
        >
          AP
        </button>

        {menuOpen && (
          <div
            style={{
              position: 'absolute', right: 0, top: 38, minWidth: 240,
              background: 'var(--surface)', border: '1px solid var(--border)',
              borderRadius: 'var(--r-md)', boxShadow: 'var(--shadow-md)',
              padding: 8, zIndex: 20,
            }}
          >
            <div style={{
              padding: '8px 10px 12px', borderBottom: '1px solid var(--divider)',
              marginBottom: 6,
            }}>
              <div style={{ fontSize: 13, fontWeight: 600, color: 'var(--ink)' }}>Apex Gateway</div>
              <div style={{ fontSize: 11, color: 'var(--muted)', fontFamily: 'var(--font-mono)', marginTop: 2 }}>
                {info ? info.listen : '—'}
              </div>
              {info && (
                <div style={{ fontSize: 11, color: 'var(--muted)', marginTop: 4 }}>
                  {info.channels} channels · {info.routers} routers · {info.teams} teams
                </div>
              )}
            </div>
            <button
              type="button"
              onClick={() => { setMenuOpen(false); window.location.hash = '#/settings' }}
              style={{
                display: 'flex', alignItems: 'center', gap: 8, width: '100%',
                padding: '7px 10px', borderRadius: 'var(--r-sm)', border: 'none',
                background: 'transparent', color: 'var(--ink)', fontSize: 13,
                cursor: 'pointer', textAlign: 'left',
              }}
              onMouseEnter={(e) => (e.currentTarget.style.background = 'var(--surface-2)')}
              onMouseLeave={(e) => (e.currentTarget.style.background = 'transparent')}
            >
              <Icon name="settings" size={13} style={{ color: 'var(--muted)' }} />
              Settings
            </button>
            <button
              type="button"
              onClick={signOut}
              style={{
                display: 'flex', alignItems: 'center', gap: 8, width: '100%',
                padding: '7px 10px', borderRadius: 'var(--r-sm)', border: 'none',
                background: 'transparent', color: 'var(--err)', fontSize: 13,
                cursor: 'pointer', textAlign: 'left',
              }}
              onMouseEnter={(e) => (e.currentTarget.style.background = 'var(--err-soft)')}
              onMouseLeave={(e) => (e.currentTarget.style.background = 'transparent')}
            >
              <Icon name="external" size={13} />
              Sign out
            </button>
          </div>
        )}
      </div>
    </div>
  )
}
