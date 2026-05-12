import { useEffect } from 'react'
import Icon from './Icon.tsx'

interface DrawerProps {
  open: boolean
  onClose: () => void
  title: string
  sub?: string
  children: React.ReactNode
  width?: number
}

export default function Drawer({ open, onClose, title, sub, children, width = 560 }: DrawerProps) {
  useEffect(() => {
    if (!open) return
    const handler = (e: KeyboardEvent) => { if (e.key === 'Escape') onClose() }
    window.addEventListener('keydown', handler)
    return () => window.removeEventListener('keydown', handler)
  }, [open, onClose])

  if (!open) return null

  return (
    <div
      style={{
        position: 'fixed', inset: 0, zIndex: 800,
        background: 'rgba(40,25,15,0.4)',
        display: 'flex', justifyContent: 'flex-end',
      }}
      onClick={onClose}
    >
      <div
        style={{
          width, maxWidth: '100vw',
          background: 'var(--surface)',
          boxShadow: 'var(--shadow-lg)',
          display: 'flex', flexDirection: 'column',
          height: '100%', overflowY: 'auto',
        }}
        onClick={(e) => e.stopPropagation()}
      >
        <div style={{
          display: 'flex', alignItems: 'flex-start', justifyContent: 'space-between',
          padding: '16px 20px', borderBottom: '1px solid var(--border)',
          position: 'sticky', top: 0, background: 'var(--surface)', zIndex: 1,
        }}>
          <div>
            <div style={{ fontWeight: 600, fontSize: 15 }}>{title}</div>
            {sub && <div style={{ fontSize: 13, color: 'var(--muted)', marginTop: 2 }}>{sub}</div>}
          </div>
          <button className="btn btn-ghost btn-sm" onClick={onClose} style={{ padding: '0 6px', marginLeft: 8 }}>
            <Icon name="x" size={16} />
          </button>
        </div>
        <div style={{ padding: '20px', flex: 1 }}>
          {children}
        </div>
      </div>
    </div>
  )
}
