// @vitest-environment jsdom
import { act } from 'react'
import { createRoot } from 'react-dom/client'
import { expect, it, vi } from 'vitest'
import BookReader from './BookReader'
const session = vi.hoisted(() => ({ save: vi.fn() }))
vi.mock('./progress', () => ({ ProgressSession: class { load = async () => null; save = session.save } }))
const epubFiles = vi.hoisted((): Record<string, string> => ({
  'META-INF/container.xml': '<container><rootfiles><rootfile full-path="OPS/package.opf"/></rootfiles></container>',
  'OPS/package.opf': '<package><manifest><item id="chapter" href="one.xhtml" media-type="application/xhtml+xml"/><item id="nav" href="nav.xhtml" properties="nav"/></manifest><spine><itemref idref="chapter"/></spine></package>',
  'OPS/nav.xhtml': '<html xmlns:epub="http://www.idpf.org/2007/ops"><body><nav epub:type="toc"><a href="one.xhtml">An actual chapter title</a></nav></body></html>',
  'OPS/one.xhtml': '<html><body><p>Chapter content</p></body></html>',
}))
vi.mock('./bookArchive', async importOriginal => ({
  ...await importOriginal<object>(),
  openArchive: async (url: string) => url.includes('epub') ? {
    entries: new Map(),
    read: async (path: string) => ({ text: async () => epubFiles[path] ?? '' }),
    close: async () => {},
  } : {
    entries: new Map([['1.png', {}], ['2.png', {}]]),
    read: async () => new Blob(['image']),
    close: async () => {},
  },
}))

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

it('shows EPUB navigation titles while preserving chapter progress locators', async () => {
  vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true)
  Object.defineProperty(HTMLDialogElement.prototype, 'showModal', { configurable: true, value: vi.fn() })
  Object.defineProperty(HTMLDialogElement.prototype, 'close', { configurable: true, value: vi.fn() })
  session.save.mockClear()
  const host = document.createElement('div'), root = createRoot(host)
  try {
    await act(async () => root.render(<BookReader sourceId="source" sourceName="Test" file={{ name: 'Book', relative_path: 'book.epub', extension: 'epub' }} onClose={() => {}}/>))
    expect(host.querySelector('select')?.textContent).toContain('An actual chapter title')
    expect(host.querySelector('iframe')?.getAttribute('sandbox')).toBe('')
    expect(session.save).toHaveBeenLastCalledWith({ position: 0, total: 1, locator: 'OPS/one.xhtml' })
    const theme = host.querySelector<HTMLSelectElement>('select[aria-label="Reading theme"]')!
    expect(theme.value).toBe('paper')
    session.save.mockClear()
    await act(async () => { theme.value = 'night'; theme.dispatchEvent(new Event('change', { bubbles: true })) })
    expect(host.querySelector('iframe')?.getAttribute('srcdoc')).toContain('background:#18191d')
    expect(host.querySelector('iframe')?.getAttribute('sandbox')).toBe('')
    expect(session.save).not.toHaveBeenCalled()
  } finally { await act(async () => root.unmount()); vi.restoreAllMocks(); vi.unstubAllGlobals() }
})
