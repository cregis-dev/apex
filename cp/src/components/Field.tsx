interface FieldProps {
  label: string
  hint?: string
  required?: boolean
  children: React.ReactNode
}

export default function Field({ label, hint, required, children }: FieldProps) {
  return (
    <div style={{ marginBottom: 16 }}>
      <label style={{
        display: 'block', fontSize: 12, fontWeight: 500,
        color: 'var(--ink-2)', marginBottom: 6,
      }}>
        {label}
        {required && <span style={{ color: 'var(--brand)', marginLeft: 3 }}>*</span>}
      </label>
      {children}
      {hint && (
        <div style={{ fontSize: 11, color: 'var(--muted)', marginTop: 5 }}>{hint}</div>
      )}
    </div>
  )
}
