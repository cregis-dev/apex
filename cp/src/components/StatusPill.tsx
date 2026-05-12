export type StatusKind = 'ok' | 'warn' | 'err' | 'info' | 'brand'

interface StatusPillProps {
  status: StatusKind
  label: string
}

export default function StatusPill({ status, label }: StatusPillProps) {
  return <span className={`badge badge-${status}`}>{label}</span>
}
