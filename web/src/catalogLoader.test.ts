import { expect, it, vi } from 'vitest'
import { loadCatalogPage } from './catalogLoader'
const personal = { items: [], has_more: false, next_offset: 0, unavailable_items: 0 }
const catalog = { items: [], truncated: false, unavailable_sources: 0 }
it('loads independent home sections after a failure without concurrent source scans', async () => {
  const calls: string[] = []
  let active = 0, maximum = 0
  const read = async <T>(url: string): Promise<T> => {
    calls.push(url); active++; maximum = Math.max(maximum, active)
    await Promise.resolve(); active--
    if (url === '/api/continue') throw new Error('Temporarily unavailable')
    return (url === '/api/catalog' ? catalog : personal) as T
  }
  const result = await loadCatalogPage('For you', new AbortController().signal, read)
  expect(calls).toEqual(['/api/continue', '/api/activity', '/api/library', '/api/catalog'])
  expect(maximum).toBe(1)
  expect(result.errors).toEqual(['Continue: Temporarily unavailable'])
  expect(result.continuing).toBeUndefined()
  expect(result.recent).toEqual(personal); expect(result.library).toEqual(personal); expect(result.catalog).toEqual(catalog)
})
it('stops requesting sections when navigation cancels a load and discards its response', async () => {
  const controller = new AbortController()
  const called = vi.fn()
  const read = async <T>(): Promise<T> => { called(); controller.abort(); return personal as T }
  const result = await loadCatalogPage('For you', controller.signal, read)
  expect(called).toHaveBeenCalledTimes(1)
  expect(result).toEqual({ errors: [], continuing: undefined, recent: undefined, library: undefined, catalog: undefined })
})
it('requests only the relevant sections for Discover and My library', async () => {
  const calls: string[] = []
  const read = async <T>(url: string): Promise<T> => { calls.push(url); return (url === '/api/catalog' ? catalog : personal) as T }
  await loadCatalogPage('Discover', new AbortController().signal, read)
  await loadCatalogPage('My library', new AbortController().signal, read)
  expect(calls).toEqual(['/api/catalog', '/api/library'])
})
