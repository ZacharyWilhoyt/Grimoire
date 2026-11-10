// @vitest-environment jsdom
import { act } from 'react'
import { createRoot } from 'react-dom/client'
import { expect, it, vi } from 'vitest'
import * as api from './api'
import SourceAgents from './SourceAgents'

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
