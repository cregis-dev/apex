interface SparklineProps {
  values: number[]
  color?: string
  width?: number
  height?: number
  fill?: boolean
}

export default function Sparkline({
  values,
  color = 'var(--brand)',
  width = 100,
  height = 28,
  fill = true,
}: SparklineProps) {
  if (values.length < 2) return null
  const min = Math.min(...values)
  const max = Math.max(...values)
  const range = max - min || 1
  const pad = 2
  const w = width
  const h = height - pad * 2
  const xs = values.map((_, i) => (i / (values.length - 1)) * w)
  const ys = values.map((v) => pad + h - ((v - min) / range) * h)
  const pts = xs.map((x, i) => `${x},${ys[i]}`).join(' ')
  const line = `M ${pts.replace(' ', ' L ')}`
  const area = `${line} L ${w},${height} L 0,${height} Z`

  return (
    <svg width={width} height={height} style={{ display: 'block', overflow: 'visible' }}>
      {fill && (
        <path d={area} fill={color} opacity={0.12} />
      )}
      <polyline
        points={pts}
        fill="none"
        stroke={color}
        strokeWidth={1.5}
        strokeLinejoin="round"
        strokeLinecap="round"
      />
    </svg>
  )
}
