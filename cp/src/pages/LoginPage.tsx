import { useState } from 'react'
import { setToken } from '../lib/auth.ts'
import Icon from '../components/Icon.tsx'

interface LoginPageProps {
  onLogin: () => void
}

export default function LoginPage({ onLogin }: LoginPageProps) {
  const [key, setKey] = useState('')
  const [persist, setPersist] = useState(false)
  const [error, setError] = useState('')
  const [loading, setLoading] = useState(false)

  async function handleSubmit(e: React.FormEvent) {
    e.preventDefault()
    if (!key.trim()) return
    setLoading(true)
    setError('')
    try {
      const res = await fetch('/api/dashboard/analytics?range=1h', {
        headers: { Authorization: `Bearer ${key.trim()}` },
      })
      if (res.status === 401 || res.status === 403) {
        setError('Invalid API key.')
      } else if (res.ok || res.status === 500) {
        // 500 means auth passed but DB might be empty — still a valid key
        setToken(key.trim(), persist)
        onLogin()
      } else {
        setError(`Unexpected response: ${res.status}`)
      }
    } catch {
      setError('Could not reach the gateway.')
    } finally {
      setLoading(false)
    }
  }

  return (
    <div style={{
      minHeight: '100vh', display: 'flex',
      alignItems: 'center', justifyContent: 'center',
      background: 'var(--bg)',
    }}>
      <div style={{ width: 360 }}>
        {/* Brand */}
        <div style={{ display: 'flex', alignItems: 'center', gap: 10, marginBottom: 32, justifyContent: 'center' }}>
          <div style={{
            width: 36, height: 36, borderRadius: 10,
            background: 'var(--brand)',
            display: 'flex', alignItems: 'center', justifyContent: 'center',
          }}>
            <Icon name="logo" size={18} style={{ color: '#fff' }} />
          </div>
          <div>
            <div style={{ fontWeight: 700, fontSize: 16, letterSpacing: '-0.01em' }}>Apex</div>
            <div style={{ fontSize: 11, letterSpacing: '0.08em', textTransform: 'uppercase', color: 'var(--muted)', fontWeight: 500 }}>Control Plane</div>
          </div>
        </div>

        <div className="card" style={{ padding: 24 }}>
          <h2 style={{ margin: '0 0 4px', fontSize: 16, fontWeight: 600 }}>Sign in</h2>
          <p style={{ margin: '0 0 20px', fontSize: 13, color: 'var(--muted)' }}>
            Enter your gateway API key to continue.
          </p>
          <form onSubmit={handleSubmit}>
            <div style={{ marginBottom: 14 }}>
              <label style={{ display: 'block', fontSize: 12, fontWeight: 500, color: 'var(--ink-2)', marginBottom: 6 }}>
                API Key
              </label>
              <input
                className="input"
                type="password"
                value={key}
                onChange={(e) => setKey(e.target.value)}
                placeholder="sk-apex-…"
                autoFocus
                style={{ width: '100%', fontFamily: 'var(--font-mono)' }}
              />
            </div>

            <label style={{ display: 'flex', alignItems: 'center', gap: 8, fontSize: 13, marginBottom: 16, cursor: 'pointer' }}>
              <input type="checkbox" checked={persist} onChange={(e) => setPersist(e.target.checked)} />
              <span style={{ color: 'var(--ink-2)' }}>Remember on this device</span>
            </label>

            {error && (
              <div style={{
                padding: '8px 12px', borderRadius: 'var(--r-sm)',
                background: 'var(--err-soft)', color: 'var(--err)',
                fontSize: 13, marginBottom: 14,
              }}>
                {error}
              </div>
            )}

            <button
              type="submit"
              className="btn btn-primary"
              disabled={loading || !key.trim()}
              style={{ width: '100%', justifyContent: 'center', height: 36 }}
            >
              {loading ? <span className="spinner" /> : 'Continue'}
            </button>
          </form>
        </div>

        <p style={{ textAlign: 'center', marginTop: 16, fontSize: 12, color: 'var(--muted)' }}>
          Leave blank if no auth keys are configured.
        </p>
      </div>
    </div>
  )
}
