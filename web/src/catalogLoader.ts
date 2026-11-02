import type { MediaItem, PageName } from './media'
export type CatalogResponse = { items: MediaItem[]; truncated: boolean; unavailable_sources: number }
export type PersonalPage = { items: MediaItem[]; has_more: boolean; next_offset: number; unavailable_items: number }
type Reader = <T>(url: string, signal: AbortSignal) => Promise<T>
export type CatalogPageResult = { continuing?: PersonalPage; recent?: PersonalPage; library?: PersonalPage; catalog?: CatalogResponse; errors: string[] }

// Keep source scans sequential: the server deliberately bounds simultaneous
// scans. A failed section must not prevent independent sections from loading.
export async function loadCatalogPage(page: PageName, signal: AbortSignal, read: Reader): Promise<CatalogPageResult> {
  const result: CatalogPageResult = { errors: [] }
  const attempt = async <T>(label: string, url: string): Promise<T | undefined> => {
    if (signal.aborted) return
    try {
      const data = await read<T>(url, signal)
      return signal.aborted ? undefined : data
    } catch (error) {
      if (!signal.aborted) result.errors.push(`${label}: ${error instanceof Error ? error.message : 'Could not load media.'}`)
      return undefined
    }
  }
  if (page === 'For you') {
    result.continuing = await attempt<PersonalPage>('Continue', '/api/continue')
    result.recent = await attempt<PersonalPage>('Recently opened', '/api/activity')
  }
  if (page !== 'Discover') result.library = await attempt<PersonalPage>('Saved library', '/api/library')
  if (page !== 'My library') result.catalog = await attempt<CatalogResponse>('Source catalog', '/api/catalog')
  return result
}
