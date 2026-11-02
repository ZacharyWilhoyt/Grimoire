// @vitest-environment jsdom
import { act } from 'react'
import { createRoot } from 'react-dom/client'
import { expect, it, vi } from 'vitest'
import App from './App'
import type { MediaItem } from './media'
const item = (key: string): MediaItem => ({ file_key: key, available: true, source_id: 'source', source_name: 'Test source', name: key, relative_path: `${key}.mp4`, kind: 'video', extension: 'mp4', size_bytes: 10, modified_at: 1, saved_at: null, last_opened_at: null })
it('renders available home sections when Continue fails and identifies the failed section', async () => {
  vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true)
  window.location.hash = '#for-you'
  const personal = (items: MediaItem[]) => ({ items, has_more: false, next_offset: 0, unavailable_items: 0 })
  vi.stubGlobal('fetch', vi.fn(async (url: string) => {
    const response = (data: unknown, status = 200) => new Response(JSON.stringify(data), { status })
    if (url === '/api/auth/me') return response({ user: { id: 'user', username: 'reader', is_admin: false }, csrf_token: 'test-csrf' })
    if (url === '/api/sources') return response([{ id: 'source', name: 'Test source', can_access_media: true, is_owner: false, available: true, acquisition_policy: 'auto_deny' }])
    if (url === '/api/continue') return response({ error: 'Progress service unavailable.' }, 503)
    if (url === '/api/activity') return response(personal([{ ...item('Recent film'), last_opened_at: 1 }]))
    if (url === '/api/library') return response(personal([{ ...item('Saved film'), saved_at: 1 }]))
    if (url === '/api/catalog') return response({ items: [item('Discovery film')], truncated: false, unavailable_sources: 0 })
    throw new Error(`Unexpected request: ${url}`)
  }))
  const host = document.createElement('div'), root = createRoot(host)
  try {
    await act(async () => root.render(<App/>))
    expect(host.textContent).toContain('Continue: Progress service unavailable.')
    for (const title of ['Recent film', 'Saved film', 'Discovery film']) expect(host.textContent).toContain(title)
    expect(host.textContent).not.toContain('Loading your media')
  } finally { await act(async () => root.unmount()); vi.unstubAllGlobals(); window.location.hash = '' }
})
it('keeps global search shortcuts out of open dialogs and unrelated form controls', async () => {
  vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true)
  window.location.hash = '#for-you'
  Object.defineProperty(HTMLDialogElement.prototype, 'showModal', { configurable: true, value: function(this: HTMLDialogElement) { this.setAttribute('open', '') } })
  Object.defineProperty(HTMLDialogElement.prototype, 'close', { configurable: true, value: function(this: HTMLDialogElement) { this.removeAttribute('open') } })
  vi.stubGlobal('fetch', vi.fn(async (url: string) => new Response(JSON.stringify(url === '/api/auth/me' ? { user: { id: 'user', username: 'reader', is_admin: false }, csrf_token: 'test-csrf' } : url === '/api/sources' ? [] : { items: [], has_more: false, next_offset: 0, unavailable_items: 0, unavailable_sources: 0, truncated: false }))))
  const host = document.createElement('div'), root = createRoot(host)
  document.body.append(host)
  const shortcut = () => new KeyboardEvent('keydown', { key: 'k', ctrlKey: true, bubbles: true, cancelable: true })
  try {
    await act(async () => root.render(<App/>))
    const search = host.querySelector<HTMLInputElement>('input[placeholder="Search your media"]') ?? host.querySelector<HTMLInputElement>('input[aria-label="Search your media"]')!
    const normal = shortcut(); window.dispatchEvent(normal)
    expect(normal.defaultPrevented).toBe(true); expect(document.activeElement).toBe(search)
    const externalInput = document.createElement('input'); host.append(externalInput); externalInput.focus()
    const editing = shortcut(); externalInput.dispatchEvent(editing)
    expect(editing.defaultPrevented).toBe(false); expect(document.activeElement).toBe(externalInput)
    externalInput.remove()
    await act(async () => [...host.querySelectorAll('button')].find(button => button.textContent === 'Account settings')!.click())
    const password = host.querySelector<HTMLInputElement>('dialog input[type="password"]')!
    password.focus()
    const modal = shortcut(); password.dispatchEvent(modal)
    expect(modal.defaultPrevented).toBe(false); expect(document.activeElement).toBe(password)
  } finally { await act(async () => root.unmount()); host.remove(); vi.unstubAllGlobals(); window.location.hash = '' }
})
