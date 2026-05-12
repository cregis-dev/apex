import { useEffect, useState } from 'react'
import { createHashRouter, RouterProvider, Navigate, Outlet } from 'react-router'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { ToastProvider } from './components/Toast.tsx'
import Sidebar from './components/Sidebar.tsx'
import LoginPage from './pages/LoginPage.tsx'
import OverviewPage from './pages/OverviewPage.tsx'
import LiveTailPage from './pages/LiveTailPage.tsx'
import RecordsPage from './pages/RecordsPage.tsx'
import ChannelsPage from './pages/ChannelsPage.tsx'
import RoutersPage from './pages/RoutersPage.tsx'
import ModelsPage from './pages/ModelsPage.tsx'
import TeamsPage from './pages/TeamsPage.tsx'
import KeysPage from './pages/KeysPage.tsx'
import RateLimitsPage from './pages/RateLimitsPage.tsx'
import SettingsPage from './pages/SettingsPage.tsx'
import { getToken, authHeaders } from './lib/auth.ts'

const qc = new QueryClient({
  defaultOptions: { queries: { retry: 1, staleTime: 30_000 } },
})

function Shell() {
  return (
    <div style={{ display: 'flex', height: '100vh', background: 'var(--bg)' }}>
      <Sidebar />
      <div style={{ flex: 1, display: 'flex', flexDirection: 'column', minWidth: 0, overflowY: 'auto' }}>
        <Outlet />
      </div>
    </div>
  )
}

const router = createHashRouter([
  {
    path: '/',
    element: <Shell />,
    children: [
      { index: true, element: <Navigate to="/overview" replace /> },
      { path: 'overview', element: <OverviewPage /> },
      { path: 'live', element: <LiveTailPage /> },
      { path: 'records', element: <RecordsPage /> },
      { path: 'channels', element: <ChannelsPage /> },
      { path: 'models', element: <ModelsPage /> },
      { path: 'routers', element: <RoutersPage /> },
      { path: 'teams', element: <TeamsPage /> },
      { path: 'keys', element: <KeysPage /> },
      { path: 'limits', element: <RateLimitsPage /> },
      { path: 'settings', element: <SettingsPage /> },
    ],
  },
])

type AuthState = 'probing' | 'ok' | 'required'

export default function App() {
  const [auth, setAuth] = useState<AuthState>(() =>
    getToken() !== null ? 'ok' : 'probing'
  )

  useEffect(() => {
    if (auth !== 'probing') return
    // Probe: if no auth keys configured on backend, 200/500 → ok. 401/403 → show login.
    fetch('/api/dashboard/analytics?range=1h', { headers: authHeaders() })
      .then((r) => setAuth(r.status === 401 || r.status === 403 ? 'required' : 'ok'))
      .catch(() => setAuth('ok')) // network error — let the app load and fail gracefully
  }, [auth])

  if (auth === 'probing') {
    return (
      <div style={{
        minHeight: '100vh', display: 'flex',
        alignItems: 'center', justifyContent: 'center',
        background: 'var(--bg)',
      }}>
        <span className="spinner" style={{ width: 20, height: 20 }} />
      </div>
    )
  }

  if (auth === 'required') {
    return (
      <ToastProvider>
        <LoginPage onLogin={() => setAuth('ok')} />
      </ToastProvider>
    )
  }

  return (
    <QueryClientProvider client={qc}>
      <ToastProvider>
        <RouterProvider router={router} />
      </ToastProvider>
    </QueryClientProvider>
  )
}
