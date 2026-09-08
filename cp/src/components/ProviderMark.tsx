export type ProviderKind =
  | 'openai' | 'anthropic' | 'deepseek' | 'ollama' | 'azure'
  | 'bedrock' | 'google' | 'mistral' | 'groq' | 'gemini'
  | 'moonshot' | 'minimax' | 'custom_dual' | 'openrouter' | 'zai' | 'jina'

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
  custom_dual: { bg: 'oklch(0.44 0.03 285)', label: 'Cd' },
  openrouter:  { bg: 'oklch(0.48 0.09 275)', label: 'Or' },
  zai:         { bg: 'oklch(0.5 0.11 255)',  label: 'Z' },
  jina:        { bg: 'oklch(0.5 0.09 95)',   label: 'Jn' },
}

/** Unknown provider: initials + a hue derived from the name, so it still reads as a mark. */
function fallback(kind: string): { bg: string; label: string } {
  const letters = kind.replace(/[^a-z0-9]/gi, '')
  const label = letters
    ? letters.slice(0, 2).charAt(0).toUpperCase() + letters.slice(1, 2)
    : '?'
  let hue = 0
  for (let i = 0; i < kind.length; i++) hue = (hue * 31 + kind.charCodeAt(i)) % 360
  return { bg: `oklch(0.47 0.07 ${hue})`, label }
}

interface ProviderMarkProps {
  kind: string
  size?: number
}

export default function ProviderMark({ kind, size = 28 }: ProviderMarkProps) {
  const p = PROVIDERS[kind] ?? fallback(kind)
  return (
    <span title={kind} style={{
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
