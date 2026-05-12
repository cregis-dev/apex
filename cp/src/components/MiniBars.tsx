interface MiniBarsProps {
  values: number[]
  color?: string
  width?: number
  height?: number
}

export default function MiniBars({ values, color = 'var(--brand)', width = 100, height = 28 }: MiniBarsProps) {
  if (!values.length) return null
  const max = Math.max(...values) || 1
  const barW = width / values.length - 1
  return (
    <svg width={width} height={height} style={{ display: 'block' }}>
      {values.map((v, i) => {
        const ratio = v / max
        const bh = Math.max(2, ratio * height)
        const opacity = 0.3 + ratio * 0.7
        return (
          <rect
            key={i}
            x={i * (barW + 1)}
            y={height - bh}
            width={barW}
            height={bh}
            fill={color}
            opacity={opacity}
            rx={1}
          />
        )
      })}
    </svg>
  )
}
