// @vitest-environment jsdom
import { expect, it, vi } from 'vitest'
import { parseXml } from './bookArchive'
import { epubChapters } from './epubChapters'
const packageXml = (nav = '') => parseXml(`<package xmlns="http://www.idpf.org/2007/opf"><manifest><item id="one" href="text/one.xhtml" media-type="application/xhtml+xml"/><item id="two" href="text/two.xhtml" media-type="application/xhtml+xml"/>${nav}</manifest><spine toc="ncx"><itemref idref="one"/><itemref idref="two"/></spine></package>`)
const archive = (files: Record<string, string>) => ({ read: vi.fn(async (path: string) => { if (!(path in files)) throw new Error('missing'); return { text: async () => files[path] } }) })
it('uses EPUB3 TOC titles without changing spine order, admitting external resources, or using landmarks', async () => {
  const book = archive({ 'OPS/nav.xhtml': '<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops"><body><nav epub:type="landmarks"><a href="text/one.xhtml">Wrong label</a></nav><nav epub:type="toc"><ol><li><a href="text/two.xhtml#start">  Second <em>section</em> </a></li><li><a href="https://outside.invalid/text/one.xhtml">External</a></li><li><a href="text/one.xhtml#intro">Introduction</a><ol><li><a href="text/one.xhtml#more">Subsection</a></li></ol></li><li><a href="not-in-spine.xhtml">Other</a></li></ol></nav></body></html>' })
  const chapters = await epubChapters(book, 'OPS/package.opf', packageXml('<item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="scripted nav"/>'))
  expect(chapters).toEqual([{ path: 'OPS/text/one.xhtml', label: 'Introduction' }, { path: 'OPS/text/two.xhtml', label: 'Second section' }])
  expect(book.read).toHaveBeenCalledTimes(1)
})
it('falls back to EPUB2 NCX, including nested entries and encoded relative filenames', async () => {
  const book = archive({ 'OPS/toc.ncx': '<ncx xmlns="http://www.daisy.org/z3986/2005/ncx/"><navMap><navPoint><navLabel><text>First</text></navLabel><content src="text/%6Fne.xhtml#start"/><navPoint><navLabel><text>Second</text></navLabel><content src="text/two.xhtml"/></navPoint></navPoint></navMap></ncx>' })
  const chapters = await epubChapters(book, 'OPS/package.opf', packageXml('<item id="nav" href="missing.xhtml" properties="nav"/><item id="ncx" href="toc.ncx" media-type="application/x-dtbncx+xml"/>'))
  expect(chapters.map(chapter => chapter.label)).toEqual(['First', 'Second'])
})
it('keeps numbered sections for missing or malformed optional navigation', async () => {
  const chapters = await epubChapters(archive({ 'OPS/nav.xhtml': '<broken>' }), 'OPS/package.opf', packageXml('<item id="nav" href="nav.xhtml" properties="nav"/>'))
  expect(chapters.map(chapter => chapter.label)).toEqual(['Chapter 1', 'Chapter 2'])
})
