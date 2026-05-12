export type ProviderKind =
  | 'openai' | 'anthropic' | 'deepseek' | 'ollama' | 'azure'
  | 'bedrock' | 'google' | 'mistral' | 'groq' | 'gemini'

const PROVIDERS: Record<string, { bg: string; label: string }> = {
  openai:    { bg: 'oklch(0.45 0.04 160)', label: 'AI' },
  anthropic: { bg: 'oklch(0.55 0.13 45)',  label: 'An' },
  deepseek:  { bg: 'oklch(0.45 0.1 250)',  label: 'Ds' },
  ollama:    { bg: 'oklch(0.32 0.02 60)',  label: 'Ol' },
  azure:     { bg: 'oklch(0.5 0.12 245)',  label: 'Az' },
  bedrock:   { bg: 'oklch(0.5 0.1 60)',    label: 'Be' },
  google:    { bg: 'oklch(0.52 0.14 25)',  label: 'Go' },
  gemini:    { bg: 'oklch(0.52 0.14 25)',  label: 'Gm' },
  mistral:   { bg: 'oklch(0.55 0.14 40)',  label: 'Mi' },
  groq:      { bg: 'oklch(0.45 0.13 25)',  label: 'Gq' },
  moonshot:  { bg: 'oklch(0.42 0.08 270)', label: 'Mo' },
  minimax:   { bg: 'oklch(0.44 0.06 200)', label: 'Mx' },
}

interface ProviderMarkProps {
  kind: string
  size?: number
}

export default function ProviderMark({ kind, size = 28 }: ProviderMarkProps) {
  const p = PROVIDERS[kind] ?? { bg: 'oklch(0.5 0.02 60)', label: '?' }
  return (
    <span style={{
      display: 'inline-flex',
      alignItems: 'center',
      justifyContent: 'center',
      width: size,
      height: size,
      borderRadius: 6,
      background: p.bg,
      color: '#fff',
      fontSize: size * 0.5,
      fontWeight: 600,
      fontFamily: 'var(--font-sans)',
      flexShrink: 0,
      letterSpacing: '-0.02em',
    }}>
      {p.label}
    </span>
  )
}
