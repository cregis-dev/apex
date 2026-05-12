import { useQuery } from '@tanstack/react-query'
import Topbar from '../components/Topbar.tsx'
import StatusPill from '../components/StatusPill.tsx'
import { api } from '../lib/api.ts'

interface RowProps { label: string; children: React.ReactNode }
function Row({ label, children }: RowProps) {
  return (
    <div style={{
      display: 'flex', alignItems: 'center', justifyContent: 'space-between',
      padding: '12px 0', borderBottom: '1px solid var(--divider)',
    }}>
      <span style={{ fontSize: 13, color: 'var(--ink-2)' }}>{label}</span>
      <span style={{ fontSize: 13 }}>{children}</span>
    </div>
  )
}

function Mono({ children }: { children: React.ReactNode }) {
  return <span style={{ fontFamily: 'var(--font-mono)', fontSize: 12, color: 'var(--muted)' }}>{children}</span>
}

interface GroupProps { title: string; children: React.ReactNode }
function Group({ title, children }: GroupProps) {
  return (
    <div className="card" style={{ marginBottom: 16 }}>
      <div style={{ padding: '14px 20px', borderBottom: '1px solid var(--border)', fontWeight: 600, fontSize: 14 }}>
        {title}
      </div>
      <div style={{ padding: '0 20px' }}>{children}</div>
    </div>
  )
}

export default function SettingsPage() {
  const { data, isLoading } = useQuery({
    queryKey: ['cpInfo'],
    queryFn: api.cpInfo,
  })

  const placeholder = <Mono>—</Mono>

  return (
    <>
      <Topbar breadcrumbs={[{ label: 'Platform' }, { label: 'Settings' }]} />
      <div className="page-pad" style={{ maxWidth: 880 }}>
        <div className="page-head">
          <h1 className="page-title">Settings</h1>
          <p className="page-sub">Gateway-level configuration, integrations, and audit.</p>
        </div>

        {isLoading && (
          <div style={{ display: 'flex', justifyContent: 'center', padding: 48 }}>
            <span className="spinner" style={{ width: 20, height: 20 }} />
          </div>
        )}

        {!isLoading && (
          <>
            <Group title="General">
              <Row label="Version">
                {data ? <Mono>v{data.version}</Mono> : placeholder}
              </Row>
              <Row label="Listen address">
                {data ? <Mono>{data.listen}</Mono> : placeholder}
              </Row>
              <Row label="Channels">
                {data ? <Mono>{data.channels}</Mono> : placeholder}
              </Row>
              <Row label="Routers">
                {data ? <Mono>{data.routers}</Mono> : placeholder}
              </Row>
              <Row label="Teams">
                {data ? <Mono>{data.teams}</Mono> : placeholder}
              </Row>
              <Row label="Hot reload">
                {data
                  ? <StatusPill status={data.hot_reload ? 'ok' : 'info'} label={data.hot_reload ? 'Enabled' : 'Disabled'} />
                  : placeholder}
              </Row>
            </Group>

            <Group title="Authentication">
              <Row label="Auth required">
                {data
                  ? <StatusPill status={data.auth_required ? 'ok' : 'warn'} label={data.auth_required ? 'Yes' : 'No'} />
                  : placeholder}
              </Row>
              <Row label="Global auth keys">
                {data ? <Mono>{data.auth_key_count} key{data.auth_key_count !== 1 ? 's' : ''} configured</Mono> : placeholder}
              </Row>
              <Row label="CORS origins">
                {data
                  ? data.cors_origins.length > 0
                    ? <div style={{ display: 'flex', gap: 6, flexWrap: 'wrap', justifyContent: 'flex-end' }}>
                        {data.cors_origins.map((o) => <span key={o} className="badge mono" style={{ fontSize: 11 }}>{o}</span>)}
                      </div>
                    : <Mono>*</Mono>
                  : placeholder}
              </Row>
            </Group>

            <Group title="Timeouts">
              <Row label="Connect timeout">
                {data ? <Mono>{data.timeouts.connect_ms} ms</Mono> : placeholder}
              </Row>
              <Row label="Request timeout">
                {data ? <Mono>{data.timeouts.request_ms} ms</Mono> : placeholder}
              </Row>
              <Row label="Response timeout">
                {data ? <Mono>{data.timeouts.response_ms} ms</Mono> : placeholder}
              </Row>
            </Group>

            <Group title="Retries">
              <Row label="Max attempts">
                {data ? <Mono>{data.retries.max_attempts}</Mono> : placeholder}
              </Row>
              <Row label="Backoff">
                {data ? <Mono>{data.retries.backoff_ms} ms</Mono> : placeholder}
              </Row>
            </Group>

            <Group title="Observability">
              <Row label="Metrics">
                {data
                  ? <StatusPill status={data.metrics_enabled ? 'ok' : 'info'} label={data.metrics_enabled ? 'Enabled' : 'Disabled'} />
                  : placeholder}
              </Row>
              <Row label="Metrics path">
                <Mono>/metrics</Mono>
              </Row>
            </Group>

            <Group title="MCP Servers">
              <div style={{ padding: '14px 0', color: 'var(--muted)', fontSize: 13 }}>
                MCP server configuration is managed via the gateway config file.
              </div>
            </Group>
          </>
        )}
      </div>
    </>
  )
}
