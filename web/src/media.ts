export type MetadataFields = { title?: string | null; creator?: string | null; collection?: string | null; year?: number | null; season?: number | null; episode?: number | null; track?: number | null; disc?: number | null; volume?: number | null; genres?: string[] | null; description?: string | null }
export type MetadataView = { fields: MetadataFields; has_custom: boolean; origin: string; artwork_url: string | null; can_edit: boolean; artwork_preference: string; provider: string | null; provider_id: string | null }
export const displayTitle = (item: { name: string; metadata?: MetadataView | null }) => item.metadata?.fields.title?.trim() || item.name
export type GroupBy = 'none' | 'collection' | 'creator'
export type MediaKind = 'video' | 'music' | 'book'
export type MediaItem = { metadata?: MetadataView | null; file_key: string; available: boolean; source_id: string; source_name: string; name: string; relative_path: string; kind: MediaKind; extension: string; size_bytes: number; modified_at: number | null; saved_at: number | null; last_opened_at: number | null }
export type PersonalState = Pick<MediaItem, 'file_key' | 'saved_at' | 'last_opened_at'>
export type PageName = 'For you' | 'Discover' | 'My library'
export type Sort = 'title' | 'modified' | 'saved' | 'source'
export const pageHashes: Record<PageName, string> = { 'For you': '#for-you', Discover: '#discover', 'My library': '#library' }
export const pageFromHash = (hash: string): PageName => (Object.entries(pageHashes).find(([, value]) => value === hash)?.[0] as PageName | undefined) ?? 'For you'
export function filterMedia(items: MediaItem[], query: string, kind: string, source: string, sort: Sort, genre = '') {
  const search = query.trim().toLowerCase()
  return items.filter(item => (!genre || item.metadata?.fields.genres?.includes(genre)) && (!kind || item.kind === kind) && (!source || item.source_id === source) && `${displayTitle(item)} ${item.name} ${item.relative_path} ${item.source_name} ${item.metadata?.fields.creator ?? ''} ${item.metadata?.fields.collection ?? ''} ${(item.metadata?.fields.genres ?? []).join(' ')}`.toLowerCase().includes(search)).sort((a, b) => {
    const order = sort === 'modified' ? (b.modified_at ?? 0) - (a.modified_at ?? 0) : sort === 'saved' ? (b.saved_at ?? 0) - (a.saved_at ?? 0) : sort === 'source' ? a.source_name.localeCompare(b.source_name) : displayTitle(a).localeCompare(displayTitle(b), undefined, { numeric: true })
    return order || a.file_key.localeCompare(b.file_key)
  })
}
// Rotate through available media types so each remains first class. These are
// transparent discovery suggestions, not an inferred taste/recommendation model.
export function suggestions(items: MediaItem[], excluded: Set<string>, limit = 9) {
  const pools = (['video', 'music', 'book'] as const).map(kind => items.filter(item => item.kind === kind && !excluded.has(item.file_key)))
  const result: MediaItem[] = []
  while (result.length < limit && pools.some(pool => pool.length)) {
    for (const pool of pools) { const item = pool.shift(); if (item && result.length < limit) result.push(item) }
  }
  return result
}
export const formatSize = (bytes: number) => bytes >= 1024 ** 3 ? `${(bytes / 1024 ** 3).toFixed(1)} GB` : bytes >= 1024 ** 2 ? `${(bytes / 1024 ** 2).toFixed(1)} MB` : `${Math.ceil(bytes / 1024)} KB`

export function groupMedia(items: MediaItem[], mode: GroupBy) {
  if (mode === 'none') return [{ key: 'all', label: '', items }]
  const groups = new Map<string, { key: string; label: string; items: MediaItem[] }>()
  const normalized = (value: string) => value.trim().toLowerCase().replace(/\s+/g, ' ')
  for (const item of items) {
    const fields = item.metadata?.fields ?? {}
    const collection = fields.collection?.trim()
    const creator = fields.creator?.trim()
    const label = mode === 'creator' ? creator : collection ? [item.kind === 'music' || item.kind === 'book' ? creator : null, collection, item.kind === 'video' && fields.season != null ? `Season ${fields.season}` : null].filter(Boolean).join(' — ') : null
    const key = `${item.kind}:${label ? normalized(label) : 'ungrouped'}`
    const group = groups.get(key) ?? { key, label: label || `Other ${item.kind === 'book' ? 'books' : item.kind}`, items: [] }
    group.items.push(item); groups.set(key, group)
  }
  return [...groups.values()].map(group => ({ ...group, items: [...group.items].sort((a,b) => {
    const left = a.metadata?.fields ?? {}, right = b.metadata?.fields ?? {}
    return (left.disc ?? 0) - (right.disc ?? 0) || (left.track ?? left.episode ?? left.volume ?? 0) - (right.track ?? right.episode ?? right.volume ?? 0) || displayTitle(a).localeCompare(displayTitle(b), undefined, { numeric: true })
  }) })).sort((a,b) => a.label.localeCompare(b.label))
}
