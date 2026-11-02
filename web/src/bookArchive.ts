import { BlobReader, ZipReader, type FileEntry } from '@zip.js/zip.js'

export const MAX_BOOK_BYTES = 128 * 1024 * 1024
const MAX_ENTRY_BYTES = 32 * 1024 * 1024
const MAX_EXPANDED_BYTES = 256 * 1024 * 1024
export const isImage = (name: string) => /\.(jpe?g|png|webp|gif|avif)$/i.test(name)

export function archivePath(base: string, reference: string) {
  const url = new URL(reference, `https://archive.invalid/${base}`)
  if (url.origin !== 'https://archive.invalid') throw new Error('External book resources are not supported.')
  return decodeURIComponent(url.pathname.slice(1))
}

export function parseXml(text: string) {
  const document = new DOMParser().parseFromString(text, 'application/xml')
  if (document.querySelector('parsererror')) throw new Error('This EPUB contains invalid XML.')
  return document
}

export async function openArchive(url: string, signal: AbortSignal) {
  const response = await fetch(url, { signal })
  if (!response.ok || !response.body) throw new Error('The book could not be opened. Check that its source is available and it is smaller than 128 MiB.')
  const chunks: Uint8Array<ArrayBuffer>[] = []
  const stream = response.body.getReader()
  let received = 0
  try {
    while (true) {
      const { value, done } = await stream.read()
      if (done) break
      received += value.byteLength
      if (received > MAX_BOOK_BYTES) throw new Error('Books must be smaller than 128 MiB.')
      chunks.push(new Uint8Array(value))
    }
  } finally { await stream.cancel() }
  const reader = new ZipReader(new BlobReader(new Blob(chunks)), { useWebWorkers: false })
  try {
    const entries = new Map<string, FileEntry>()
    let expanded = 0
    let entryCount = 0
    for await (const entry of reader.getEntriesGenerator()) {
      if (++entryCount > 5000) throw new Error('This archive contains too many files.')
      if (entry.directory) continue
      expanded += entry.uncompressedSize
      if (entry.encrypted || entry.uncompressedSize > MAX_ENTRY_BYTES || expanded > MAX_EXPANDED_BYTES) throw new Error('This book is encrypted or exceeds the reader archive limits.')
      if (entries.has(entry.filename)) throw new Error('This archive contains duplicate file names.')
      entries.set(entry.filename, entry)
    }
    let actualExpanded = 0
    const cache = new Map<string, Promise<Blob>>()
    const extract = async (name: string) => {
      const entry = entries.get(name)
      if (!entry) throw new Error('A required book resource is missing.')
      const parts: Uint8Array<ArrayBuffer>[] = []
      let size = 0
      await entry.getData(new WritableStream<Uint8Array>({ write(chunk) {
        size += chunk.byteLength
        actualExpanded += chunk.byteLength
        if (actualExpanded > MAX_EXPANDED_BYTES) throw new Error('This book exceeds the expanded size limit.')
        if (size > MAX_ENTRY_BYTES) throw new Error('A book resource exceeds the reader size limit.')
        parts.push(new Uint8Array(chunk))
      } }), { signal, checkSignature: true })
      return new Blob(parts)
    }
    const read = (name: string) => {
      let result = cache.get(name)
      if (!result) { result = extract(name); cache.set(name, result) }
      return result
    }
    return { entries, read, close: () => reader.close() }
  } catch (error) { await reader.close(); throw error }
}
export type BookArchive = Awaited<ReturnType<typeof openArchive>>
