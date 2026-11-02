import { archivePath, parseXml } from './bookArchive'

export type Chapter = { path: string; label: string }
type Archive = { read: (path: string) => Promise<{ text: () => Promise<string> }> }
const elements = (node: Document | Element, localName: string) => [...node.getElementsByTagNameNS('*', localName)]
const title = (value: string | null | undefined) => (value ?? '').replace(/\s+/g, ' ').trim().slice(0, 160)

// Navigation is optional metadata. It never changes the reading order or adds
// resources outside the package spine, and malformed TOCs do not block reading.
export async function epubChapters(archive: Archive, packagePath: string, packageXml: Document): Promise<Chapter[]> {
  const manifestElement = elements(packageXml, 'manifest')[0]
  const manifest = new Map(manifestElement ? elements(manifestElement, 'item').map(item => [item.getAttribute('id'), item]) : [])
  const spine = elements(packageXml, 'spine')[0]
  const pages = (spine ? elements(spine, 'itemref') : []).filter(item => item.getAttribute('linear') !== 'no').map((item, index) => {
    const entry = manifest.get(item.getAttribute('idref'))
    if (!entry || entry.getAttribute('media-type') !== 'application/xhtml+xml') throw new Error('This EPUB has an unsupported chapter format.')
    return { path: archivePath(packagePath, entry.getAttribute('href') ?? ''), label: `Chapter ${index + 1}` }
  })
  const labels = new Map<string, string>()
  const allowed = new Set(pages.map(page => page.path))
  const add = (base: string, href: string | null, text: string | null | undefined) => {
    if (!href) return
    try {
      const path = archivePath(base, href), label = title(text)
      if (allowed.has(path) && label && !labels.has(path)) labels.set(path, label)
    } catch { /* External or invalid links are not book navigation. */ }
  }
  const navItem = [...manifest.values()].find(item => item.getAttribute('properties')?.split(/\s+/).includes('nav'))
  if (navItem) {
    try {
      const path = archivePath(packagePath, navItem.getAttribute('href') ?? '')
      const document = parseXml(await (await archive.read(path)).text())
      const navs = elements(document, 'nav')
      const toc = navs.find(nav => (nav.getAttributeNS('http://www.idpf.org/2007/ops', 'type') ?? nav.getAttribute('epub:type') ?? '').split(/\s+/).includes('toc'))
      if (toc) for (const anchor of elements(toc, 'a')) add(path, anchor.getAttribute('href'), anchor.textContent)
    } catch { /* Fall back to NCX or numbered sections. */ }
  }
  const ncxItem = manifest.get(spine?.getAttribute('toc') ?? '') ?? [...manifest.values()].find(item => item.getAttribute('media-type') === 'application/x-dtbncx+xml')
  if (ncxItem && pages.some(page => !labels.has(page.path))) {
    try {
      const path = archivePath(packagePath, ncxItem.getAttribute('href') ?? '')
      const document = parseXml(await (await archive.read(path)).text())
      for (const point of elements(document, 'navPoint')) {
        const content = [...point.children].find(child => child.localName === 'content')
        const label = [...point.children].find(child => child.localName === 'navLabel')
        add(path, content?.getAttribute('src') ?? null, label?.textContent)
      }
    } catch { /* Navigation metadata must not make an otherwise readable book fail. */ }
  }
  return pages.map(page => ({ ...page, label: labels.get(page.path) ?? page.label }))
}
