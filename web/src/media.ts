export type MediaKind = 'video' | 'music' | 'book'
export type MediaItem = { file_key: string; available: boolean; source_id: string; source_name: string; name: string; relative_path: string; kind: MediaKind; extension: string; size_bytes: number; modified_at: number | null; saved_at: number | null; last_opened_at: number | null }
export type PersonalState = Pick<MediaItem, 'file_key' | 'saved_at' | 'last_opened_at'>
export type PageName = 'For you' | 'Discover' | 'My library'
export type Sort = 'title' | 'modified' | 'saved' | 'source'
export const pageHashes: Record<PageName, string> = { 'For you': '#for-you', Discover: '#discover', 'My library': '#library' }
export const pageFromHash = (hash: string): PageName => (Object.entries(pageHashes).find(([, value]) => value === hash)?.[0] as PageName | undefined) ?? 'For you'
export function filterMedia(items: MediaItem[], query: string, kind: string, source: string, sort: Sort) {
  const search = query.trim().toLowerCase()
  return items.filter(item => (!kind || item.kind === kind) && (!source || item.source_id === source) && `${item.name} ${item.relative_path} ${item.source_name}`.toLowerCase().includes(search)).sort((a, b) => {
    const order = sort === 'modified' ? (b.modified_at ?? 0) - (a.modified_at ?? 0) : sort === 'saved' ? (b.saved_at ?? 0) - (a.saved_at ?? 0) : sort === 'source' ? a.source_name.localeCompare(b.source_name) : a.name.localeCompare(b.name, undefined, { numeric: true })
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
