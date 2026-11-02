import { afterEach, describe, expect, it, vi } from 'vitest'
import { BlobWriter, TextReader, ZipWriter } from '@zip.js/zip.js'
import { archivePath, openArchive } from './bookArchive'

afterEach(() => vi.unstubAllGlobals())
async function fixture() {
  const writer = new ZipWriter(new BlobWriter(), { useWebWorkers: false, level: 0 })
  await writer.add('chapter.xhtml', new TextReader('<p>Hello</p>'))
  return writer.close()
}
function mockBook(blob: Blob) { vi.stubGlobal('fetch', vi.fn(async () => new Response(blob))) }

describe('book archives', () => {
  it('resolves EPUB resources while rejecting external origins', () => {
    expect(archivePath('OPS/text/chapter.xhtml', '../images/cover.png')).toBe('OPS/images/cover.png')
    expect(() => archivePath('OPS/book.opf', 'https://example.com/book')).toThrow()
    expect(() => archivePath('OPS/book.opf', 'javascript:alert(1)')).toThrow()
  })
  it('reads archive resources and rejects missing entries', async () => {
    mockBook(await fixture())
    const book = await openArchive('/book', new AbortController().signal)
    try {
      expect(await (await book.read('chapter.xhtml')).text()).toBe('<p>Hello</p>')
      await expect(book.read('missing')).rejects.toThrow('missing')
    } finally { await book.close() }
  })
  it('rejects oversized expanded entries before extraction', async () => {
    const bytes = new Uint8Array(await (await fixture()).arrayBuffer())
    const view = new DataView(bytes.buffer)
    for (let i = 0; i < bytes.length - 46; i++) {
      if (view.getUint32(i, true) === 0x02014b50) { view.setUint32(i + 24, 33 * 1024 * 1024, true); break }
    }
    mockBook(new Blob([bytes]))
    await expect(openArchive('/book', new AbortController().signal)).rejects.toThrow('limits')
  })
  it('reports invalid archives', async () => {
    mockBook(new Blob(['not a zip file']))
    await expect(openArchive('/book', new AbortController().signal)).rejects.toThrow()
  })
})
