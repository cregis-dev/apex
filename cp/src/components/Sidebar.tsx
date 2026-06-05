import { NavLink, useLocation } from 'react-router'
import { useQuery } from '@tanstack/react-query'
import Icon, { type IconName } from './Icon.tsx'
import { api } from '../lib/api.ts'

interface NavItem {
  id: string
  label: string
  icon: IconName
  badge?: string
  live?: boolean
}

interface NavSection {
  section: string
  items: NavItem[]
}

const NAV: NavSection[] = [
  {
    section: 'Operate',
    items: [
      { id: 'overview', label: 'Overview', icon: 'home' },
      { id: 'live', label: 'Live Tail', icon: 'activity', live: true },
      { id: 'records', label: 'Records', icon: 'list' },
    ],
  },
  {
    section: 'Configure',
    items: [
      { id: 'channels', label: 'Channels', icon: 'plug' },
      { id: 'models', label: 'Models', icon: 'cube' },
      { id: 'routers', label: 'Routers', icon: 'route' },
    ],
  },
  {
    section: 'Access',
    items: [
      { id: 'teams', label: 'Teams', icon: 'users' },
      { id: 'limits', label: 'Rate Limits', icon: 'gauge' },
    ],
  },
  {
    section: 'Platform',
    items: [
      { id: 'settings', label: 'Settings', icon: 'settings' },
    ],
  },
]

export default function Sidebar() {
  const location = useLocation()
  const { data: info } = useQuery({
    queryKey: ['cpInfo'],
    queryFn: api.cpInfo,
    staleTime: 60_000,
  })

  return (
    <nav style={{
      width: 232, flexShrink: 0,
      background: 'var(--bg-soft)',
      borderRight: '1px solid var(--border)',
      display: 'flex', flexDirection: 'column',
      position: 'sticky', top: 0, height: '100vh',
      overflowY: 'auto',
    }}>
      {/* Brand */}
      <div style={{ padding: '18px 16px 16px', borderBottom: '1px solid var(--border)' }}>
        <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
          <div style={{
            width: 32, height: 32, borderRadius: 8,
            background: 'var(--brand)', display: 'flex',
            alignItems: 'center', justifyContent: 'center',
          }}>
            <Icon name="logo" size={16} style={{ color: '#fff' }} />
          </div>
          <div>
            <div style={{ fontWeight: 700, fontSize: 14, letterSpacing: '-0.01em', lineHeight: 1.2 }}>Apex</div>
            <div style={{ fontSize: 10, letterSpacing: '0.08em', textTransform: 'uppercase', color: 'var(--muted)', fontWeight: 500 }}>Control Plane</div>
          </div>
        </div>
      </div>

      {/* Nav sections */}
      <div style={{ flex: 1, padding: '8px 8px' }}>
        {NAV.map((section) => (
          <div key={section.section} style={{ marginBottom: 8 }}>
            <div style={{
              fontSize: 10.5, fontWeight: 500, textTransform: 'uppercase',
              letterSpacing: '0.06em', color: 'var(--muted)',
              padding: '10px 8px 4px',
            }}>
              {section.section}
            </div>
            {section.items.map((item) => {
              const active = location.pathname === `/${item.id}`
              return (
                <NavLink
                  key={item.id}
                  to={`/${item.id}`}
                  style={{
                    display: 'flex', alignItems: 'center', gap: 9,
                    padding: '7px 10px', borderRadius: 'var(--r-sm)',
                    fontSize: 13, fontWeight: active ? 500 : 400,
                    color: active ? 'var(--ink)' : 'var(--ink-2)',
                    background: active ? 'var(--surface)' : 'transparent',
                    boxShadow: active ? 'var(--shadow-xs)' : 'none',
                    border: active ? '1px solid var(--border)' : '1px solid transparent',
                    marginBottom: 1, textDecoration: 'none',
                    transition: 'background 100ms',
                  }}
                >
                  <Icon
                    name={item.icon}
                    size={15}
                    style={{ color: active ? 'var(--brand)' : 'var(--muted)', flexShrink: 0 }}
                  />
                  <span style={{ flex: 1 }}>{item.label}</span>
                  {item.live && (
                    <span style={{
                      width: 6, height: 6, borderRadius: '50%',
                      background: 'var(--err)',
                      animation: 'blink-rec 1.4s ease-in-out infinite',
                    }} />
                  )}
                </NavLink>
              )
            })}
          </div>
        ))}
      </div>

      {/* Health footer */}
      <div style={{
        padding: '12px 16px',
        borderTop: '1px solid var(--border)',
        display: 'flex', alignItems: 'center', gap: 8,
      }}>
        <span className="dot dot-ok" />
        <span style={{ fontSize: 12, color: 'var(--muted)', flex: 1 }}>All systems normal</span>
        {info && <span className="mono muted" style={{ fontSize: 11 }}>v{info.version}</span>}
      </div>
    </nav>
  )
}
