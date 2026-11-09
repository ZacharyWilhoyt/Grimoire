export type MetadataFields = { duration?: number | null; title?: string | null; creator?: string | null; collection?: string | null; year?: number | null; season?: number | null; episode?: number | null; track?: number | null; disc?: number | null; volume?: number | null; genres?: string[] | null; description?: string | null }
export type GroupView = { id: string; label: string; can_edit: boolean; has_custom_genres: boolean; has_custom_artwork: boolean; synopsis?: string | null; synopsis_provider?: string | null }
export type MetadataView = { group?: GroupView | null; individual_genres?: string[] | null; credits?: string[]; fields: MetadataFields; has_custom: boolean; origin: string; artwork_url: string | null; can_edit: boolean; artwork_preference: string; provider: string | null; provider_id: string | null }
export const displayTitle = (item: { name: string; metadata?: MetadataView | null }) => item.metadata?.fields.title?.trim() || item.name
export type GroupBy = 'none' | 'collection' | 'creator'
export type MediaKind = 'video' | 'music' | 'book'
export type MediaItem = { progress?: { position: number; total: number | null; updated_at: number; completed?:boolean } | null; metadata?: MetadataView | null; file_key: string; available: boolean; source_id: string; source_name: string; name: string; relative_path: string; kind: MediaKind; extension: string; size_bytes: number; modified_at: number | null; saved_at: number | null; last_opened_at: number | null }
export type PersonalState = Pick<MediaItem, 'file_key' | 'saved_at' | 'last_opened_at'>
export type PageName = 'For you' | 'Discover' | 'My library' | 'Music'
export type Sort = 'title' | 'modified' | 'saved' | 'source'
export const pageHashes: Record<PageName, string> = { 'For you': '#for-you', Discover: '#discover', 'My library': '#library', Music: '#music' }
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

export type MediaGroup = { key: string; label: string; items: MediaItem[]; folder: boolean }
export function orderMembers(items: MediaItem[]) {
  return [...items].sort((a,b) => {
    const left = a.metadata?.fields ?? {}, right = b.metadata?.fields ?? {}
    return (left.season ?? 0) - (right.season ?? 0) || (left.disc ?? 0) - (right.disc ?? 0) || (left.track ?? left.episode ?? left.volume ?? 0) - (right.track ?? right.episode ?? right.volume ?? 0) || displayTitle(a).localeCompare(displayTitle(b), undefined, { numeric: true })
  })
}
export function groupMedia(items: MediaItem[], mode: GroupBy, preserveOrder = false): MediaGroup[] {
  if (mode === 'none') return [{ key: 'all', label: '', items, folder: false }]
  const groups = new Map<string, MediaGroup>()
  const normalized = (value: string) => value.trim().toLowerCase().replace(/\s+/g, ' ')
  for (const item of items) {
    const fields = item.metadata?.fields ?? {}
    const collection = fields.collection?.trim(), creator = fields.creator?.trim()
    const label = mode === 'creator' ? creator : collection ? [item.kind === 'music' || item.kind === 'book' ? creator : null, collection].filter(Boolean).join(' — ') : null
    const identity = mode === 'collection' ? item.metadata?.group?.id : null
    const key = label ? identity || JSON.stringify([item.source_id, item.kind, mode, normalized(label)]) : `file:${item.file_key}`
    const group = groups.get(key) ?? { key, label: label || '', items: [], folder: !!label }
    group.items.push(item); groups.set(key, group)
  }
  const result = [...groups.values()].map(group => ({ ...group, items: group.folder ? orderMembers(group.items) : group.items }))
  return preserveOrder ? result : result.sort((a,b) => (a.label || displayTitle(a.items[0])).localeCompare(b.label || displayTitle(b.items[0]), undefined, { numeric: true }))
}
export function subdivisions(items: MediaItem[]): MediaGroup[] | null {
  const kind = items[0]?.kind
  if (kind !== 'video' && kind !== 'music') return null
  const field = kind === 'video' ? 'season' : 'disc'
  const numbers = [...new Set(items.map(item => item.metadata?.fields[field] ?? null))]
  // TV seasons are useful even when just one season is currently visible.
  if (!numbers.some(number => number != null) || kind === 'music' && numbers.filter(number => number != null).length < 2) return null
  return numbers.sort((a,b) => (a ?? -1) - (b ?? -1)).map(number => ({ key: `${field}:${number}`, label: number == null ? 'Unnumbered' : `${kind === 'video' ? 'Season' : 'Disc'} ${number}`, items: orderMembers(items.filter(item => (item.metadata?.fields[field] ?? null) === number)), folder: true }))
}

export function progressLabel(item: MediaItem) {
  const progress = item.progress
  if (!progress || progress.position <= 0) return ''
  if (item.kind === 'book') return `${item.extension.toLowerCase() === 'cbz' ? 'Page' : 'Chapter'} ${Math.floor(progress.position) + 1}${progress.total ? ` of ${progress.total}` : ''}`
  const seconds = Math.floor(progress.position), hours = Math.floor(seconds / 3600)
  const time = hours ? `${hours}:${String(Math.floor(seconds / 60) % 60).padStart(2,'0')}:${String(seconds % 60).padStart(2,'0')}` : `${Math.floor(seconds / 60)}:${String(seconds % 60).padStart(2,'0')}`
  return `Saved at ${time}`
}
