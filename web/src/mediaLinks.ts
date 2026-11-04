import type { MediaItem } from './media'
export type LinkContext = { item: MediaItem | null; members: MediaItem[]; label: string | null }
export function mediaLink(source: string, path: string) { return `/?${new URLSearchParams({ source, item: path })}` }
export function detailLink(source:string,path:string){return `/?${new URLSearchParams({source,details:path})}`}
export function seriesLink(source: string, group: string) { return `/?${new URLSearchParams({ source, series: group })}` }
export function linkRequest(search: string) {
  const params = new URLSearchParams(search), source = params.get('source'), path = params.get('item')??params.get('details'), group = params.get('series')
  if (!source || (!path && !group)) return null
  return `/api/media/link?${new URLSearchParams({ source_id: source, ...(path ? { path } : { group: group! }) })}`
}
