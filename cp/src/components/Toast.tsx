import { createContext, useCallback, useContext, useState } from 'react'

type ToastKind = 'info' | 'ok'

interface ToastItem {
  id: number
  msg: string
  kind: ToastKind
}

interface ToastCtx {
  push: (msg: string, kind?: ToastKind) => void
}

const Ctx = createContext<ToastCtx>({ push: () => {} })

let _id = 0

export function ToastProvider({ children }: { children: React.ReactNode }) {
  const [toasts, setToasts] = useState<ToastItem[]>([])

  const push = useCallback((msg: string, kind: ToastKind = 'info') => {
    const id = ++_id
    setToasts((t) => [...t, { id, msg, kind }])
    setTimeout(() => setToasts((t) => t.filter((x) => x.id !== id)), 2800)
  }, [])

  return (
    <Ctx.Provider value={{ push }}>
      {children}
      <div style={{
        position: 'fixed', bottom: 24, right: 24,
        display: 'flex', flexDirection: 'column', gap: 8,
        zIndex: 1000, pointerEvents: 'none',
      }}>
        {toasts.map((t) => (
          <div key={t.id} style={{
            background: 'var(--ink)', color: '#fff',
            padding: '10px 14px', borderRadius: 'var(--r-md)',
            fontSize: 13, fontWeight: 500,
            boxShadow: 'var(--shadow-lg)',
            animation: 'slide-in-row 220ms ease',
            pointerEvents: 'auto',
            display: 'flex', alignItems: 'center', gap: 8,
          }}>
            {t.kind === 'ok' && (
              <span style={{ color: 'var(--ok)', fontSize: 16 }}>✓</span>
            )}
            {t.msg}
          </div>
        ))}
      </div>
    </Ctx.Provider>
  )
}

export function useToast() {
  return useContext(Ctx)
}
