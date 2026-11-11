import type { MediaItem } from './media'
export type SeriesArtwork = { banner_url: string | null; poster_url: string | null }
export type SeriesSeason = { key: string; count: number; poster_url: string | null }
export type LinkContext = {
  item: MediaItem | null
  members: MediaItem[]
  label: string | null
  artwork?: SeriesArtwork
  seasons?: SeriesSeason[]
  season?: string | null
  series_source_id?: string
  series_group_id?: string
}
export function mediaLink(source: string, path: string) { return `/?${new URLSearchParams({ source, item: path })}` }
export function detailLink(source:string,path:string){return `/?${new URLSearchParams({source,details:path})}`}
export function seriesLink(source: string, group: string, season?: string | null) {
  return `/?${new URLSearchParams({ source, series: group, ...(season != null ? { season } : {}) })}`
}
export function linkRequest(search: string) {
  const params = new URLSearchParams(search), source = params.get('source'), path = params.get('item')??params.get('details'), group = params.get('series')
  if (!source || (!path && !group)) return null
  const season = params.get('season')
  return `/api/media/link?${new URLSearchParams({ source_id: source, ...(path ? { path } : { group: group!, ...(season != null ? { season } : {}) }) })}`
}
