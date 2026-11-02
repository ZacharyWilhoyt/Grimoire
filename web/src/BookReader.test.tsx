// @vitest-environment jsdom
import { act } from 'react'
import { createRoot } from 'react-dom/client'
import { expect, it, vi } from 'vitest'
import BookReader from './BookReader'
const session = vi.hoisted(() => ({ save: vi.fn() }))
vi.mock('./progress', () => ({ ProgressSession: class { load = async () => null; save = session.save } }))
vi.mock('./bookArchive', async importOriginal => ({ ...await importOriginal<object>(), openArchive: async () => ({ entries: new Map([['1.png', {}], ['2.png', {}]]), read: async () => new Blob(['image']), close: async () => {} }) }))
it('saves a comic page only after its image loads and does not save a damaged image', async () => {
  vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true)
  Object.defineProperty(HTMLDialogElement.prototype, 'showModal', { configurable: true, value: vi.fn() })
  Object.defineProperty(HTMLDialogElement.prototype, 'close', { configurable: true, value: vi.fn() })
  URL.createObjectURL = vi.fn(() => 'blob:test'); URL.revokeObjectURL = vi.fn()
  session.save.mockClear()
  const host = document.createElement('div'), root = createRoot(host)
  try {
    await act(async () => root.render(<BookReader sourceId="source" sourceName="Test" file={{ name: 'Comic', relative_path: 'comic.cbz', extension: 'cbz' }} onClose={() => {}}/>))
    const first = host.querySelector('img')!
    expect(first).not.toBeNull(); expect(session.save).not.toHaveBeenCalled()
    await act(async () => first.dispatchEvent(new Event('load')))
    expect(session.save).toHaveBeenLastCalledWith({ position: 0, total: 2, locator: '1.png' })
    session.save.mockClear()
    await act(async () => host.querySelector<HTMLButtonElement>('button[aria-label="Next"]')!.click())
    expect(session.save).not.toHaveBeenCalled()
    await act(async () => host.querySelector('img')!.dispatchEvent(new Event('error')))
    expect(host.textContent).toContain('damaged or unsupported')
    expect(session.save).not.toHaveBeenCalled()
  } finally { await act(async () => root.unmount()); vi.restoreAllMocks(); vi.unstubAllGlobals() }
})
