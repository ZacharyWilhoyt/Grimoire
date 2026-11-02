export type RepeatMode = 'off' | 'all' | 'one'
export function playOrder(count: number, current: number, shuffle: boolean, random = Math.random): number[] {
  const indices = Array.from({ length: count }, (_,index) => index)
  if (!shuffle) return indices
  const rest = indices.filter(index => index !== current)
  for (let i = rest.length - 1; i > 0; i--) {
    const other = Math.floor(random() * (i + 1))
    ;[rest[i], rest[other]] = [rest[other], rest[i]]
  }
  return indices.includes(current) ? [current, ...rest] : rest
}
export function adjacentTrack(order: number[], current: number, direction: 1 | -1, repeat: RepeatMode): number | undefined {
  const position = order.indexOf(current)
  if (position < 0 || !order.length) return undefined
  const next = position + direction
  if (next >= 0 && next < order.length) return order[next]
  return repeat === 'all' ? order[(next + order.length) % order.length] : undefined
}
