// @vitest-environment jsdom
import { act } from 'react'
import { createRoot } from 'react-dom/client'
import { expect, it, vi } from 'vitest'
import * as api from './api'
import SourceAgents from './SourceAgents'
import App from './App'

vi.mock('./Authentication', () => ({
  default: ({ children }: { children: (user: unknown, logout: () => void) => unknown }) =>
    children({ id: 'owner', username: 'Owner', is_admin: false }, () => {}),
}))

it('opens an authorized remote book and keeps the enrollment token in the dialog', async () => {
  vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true)
  Object.defineProperty(HTMLDialogElement.prototype, 'showModal', { configurable: true, value: vi.fn() })
  Object.defineProperty(HTMLDialogElement.prototype, 'close', { configurable: true, value: vi.fn() })
  const book = { id: 'item', representation_id: 'representation', name: 'Book.epub', extension: 'epub', size_bytes: 99 }
  const fetch = vi.spyOn(api, 'apiFetch').mockImplementation(async path => {
    if (path.endsWith('enrollment-tokens')) return Response.json({ token: 'disposable-token' })
    if (path.endsWith('/catalog')) return Response.json({ items: [book], can_access_media: true, next: null })
    if (path.endsWith('/sources')) return Response.json([{ id: 'source', name: 'Remote library', is_owner: true, available: true }])
    return Response.json([])
  })
  const host = document.createElement('div'), root = createRoot(host)
  const openBook = vi.fn()
  const button = (label: string) => [...host.querySelectorAll('button')].find(button => button.textContent === label)!
  try {
    await act(async () => root.render(<SourceAgents onClose={() => {}} onPermissions={() => {}} onBook={openBook}/>))
    await act(async () => button('Create enrollment token').click())
    expect(host.querySelector('input')?.value).toBe('disposable-token')
    await act(async () => button('Remote library').click())
    await act(async () => button('Read').click())
    expect(openBook).toHaveBeenCalledWith({
      item: book, sourceId: 'source', sourceName: 'Remote library',
      url: '/api/nodes/v1/sources/source/representations/representation',
    })
  } finally {
    await act(async () => root.unmount())
    fetch.mockRestore()
    vi.unstubAllGlobals()
  }
})


it('lists an enrolled agent in Your Sources and opens its published library', async () => {
  vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true)
  Object.defineProperty(HTMLDialogElement.prototype, 'showModal', { configurable: true, value: vi.fn() })
  Object.defineProperty(HTMLDialogElement.prototype, 'close', { configurable: true, value: vi.fn() })
  const fetch = vi.spyOn(api, 'apiFetch').mockImplementation(async path => {
    if (path === '/api/nodes/v1') return Response.json([
      { id: 'panda', label: 'Panda', online: true, revoked: false },
      { id: 'retired', label: 'Retired', online: false, revoked: true },
    ])
    if (path === '/api/nodes/v1/sources') return Response.json([
      { id: 'panda-source', node_id: 'panda', name: 'Panda library', is_owner: true, available: true },
      { id: 'other-source', node_id: 'other', name: 'Other library', is_owner: true, available: true },
    ])
    if (path === '/api/nodes/v1/sources/panda-source/catalog') return Response.json({
      items: [{ id: 'item', representation_id: 'representation', name: 'Panda book.epub', extension: 'epub', size_bytes: 99 }],
      can_access_media: true, next: null,
    })
    if (path === '/api/sources') return Response.json([])
    return Response.json({ items: [], has_more: false, next_offset: 0, unavailable_items: 0 })
  })
  const host = document.createElement('div'), root = createRoot(host)
  try {
    await act(async () => root.render(<App/>))
    const sidebar = host.querySelector('.sidebar')!
    const agent = [...sidebar.querySelectorAll('button.source-row')].find(button => button.textContent?.includes('Panda')) as HTMLButtonElement
    expect(agent).toBeDefined()
    expect(sidebar.textContent).not.toContain('Retired')
    await act(async () => agent.click())
    const panel = host.querySelector('.source-agent-dialog')!
    expect(panel.textContent).toContain('Panda book.epub')
    expect(panel.textContent).not.toContain('Other library')
  } finally {
    await act(async () => root.unmount())
    fetch.mockRestore()
    vi.unstubAllGlobals()
  }
})
