import MetadataCredits from './MetadataCredits'
import WatchSidebar from './WatchSidebar'
import { epubChapters, type Chapter } from './epubChapters'
import { ProgressSession } from './progress'
import { displayTitle, type MetadataView } from './media'
import { useEffect, useRef, useState } from 'react'
import { sanitizeChapter, chapterDocument, type ReadingTheme } from './epubContent'
import { ChevronLeft, ChevronRight, ArrowLeft } from 'lucide-react'
import { archivePath, isImage, openArchive, parseXml, type BookArchive } from './bookArchive'

type Props = { sourceId: string; sourceName: string; file: { metadata?: MetadataView | null; name: string; relative_path: string; extension: string }; onClose: () => void }

export default function BookReader({ sourceId, sourceName, file, onClose }: Props) {
  const progress = useRef<ProgressSession | null>(null)
  const [progressReady, setProgressReady] = useState(false)
  const [progressError, setProgressError] = useState('')
  const [archive, setArchive] = useState<BookArchive | null>(null)
  const [chapters, setChapters] = useState<Chapter[]>([])
  const [renderedIndex, setRenderedIndex] = useState(-1)
  const [index, setIndex] = useState(0)
  const [zoom, setZoom] = useState(100)
  const [fontSize, setFontSize] = useState(19)
  const [theme, setTheme] = useState<ReadingTheme>('paper')
  const [content, setContent] = useState('')
  const [image, setImage] = useState('')
  const [error, setError] = useState('')
  const [loading, setLoading] = useState(true)
  const comic = file.extension.toLowerCase() === 'cbz'

  useEffect(() => {
    setProgressReady(false); setProgressError(''); setArchive(null); setChapters([]); setLoading(true); setRenderedIndex(-1)
    const controller = new AbortController()
    let opened: BookArchive | undefined
    const load = async () => {
      const session = new ProgressSession(sourceId, file.relative_path, message => { if (!controller.signal.aborted) setProgressError(message) })
      progress.current = session
      const saved = await session.load()
      if (controller.signal.aborted) return
      opened = await openArchive(`/api/sources/${encodeURIComponent(sourceId)}/book?${new URLSearchParams({ path: file.relative_path })}`, controller.signal)
      if (controller.signal.aborted) { await opened.close(); return }
      let pages: Chapter[]
      if (comic) {
        pages = [...opened.entries.keys()].filter(isImage).sort((a, b) => a.localeCompare(b, undefined, { numeric: true })).map(path => ({ path, label: path }))
      } else {
        const container = parseXml(await (await opened.read('META-INF/container.xml')).text())
        const packagePath = container.querySelector('rootfile')?.getAttribute('full-path')
        if (!packagePath) throw new Error('The EPUB package was not found.')
        const packageXml = parseXml(await (await opened.read(packagePath)).text())
        pages = await epubChapters(opened, packagePath, packageXml)
      }
      if (!pages.length) throw new Error('No readable pages were found in this book.')
      if (!controller.signal.aborted) {
        const located = saved?.locator ? pages.findIndex(page => page.path === saved.locator) : -1
        setIndex(located >= 0 ? located : Math.min(Math.max(0, Math.floor(saved?.position ?? 0)), pages.length - 1))
        setArchive(opened); setChapters(pages); setProgressReady(true)
      }
    }
    load().catch(error => { if (!controller.signal.aborted) { setError(error instanceof Error ? error.message : 'Could not open this book.'); setLoading(false) } })
    return () => { controller.abort(); void opened?.close().catch(() => {}) }
  }, [sourceId, file.relative_path, comic])

  useEffect(() => {
    if (!archive || !chapters[index]) return
    let cancelled = false
    const urls: string[] = []
    const objectUrl = (blob: Blob) => { const url = URL.createObjectURL(blob); urls.push(url); return url }
    setLoading(true); setRenderedIndex(-1); setError(''); setImage(''); setContent('')
    const load = async () => {
      const chapter = chapters[index]
      const blob = await archive.read(chapter.path)
      if (cancelled) return
      if (comic) { setImage(objectUrl(blob)); return }
      const xml = parseXml(await blob.text())
      const body = xml.querySelector('body')
      if (!body) throw new Error('This chapter has no readable content.')
      const fragment = sanitizeChapter(new XMLSerializer().serializeToString(body))
      for (const image of fragment.querySelectorAll('img')) {
        const reference = image.getAttribute('src')
        image.removeAttribute('src')
        if (!reference) continue
        try {
          const path = archivePath(chapter.path, reference)
          if (isImage(path) && archive.entries.has(path)) {
            const imageBlob = await archive.read(path)
            if (cancelled) return
            image.src = objectUrl(imageBlob)
          }
        } catch { /* Omit external or missing images. */ }
      }
      for (const link of fragment.querySelectorAll('a')) link.removeAttribute('href')
      const wrapper = document.createElement('div'); wrapper.append(fragment)
      if (!cancelled) { setContent(wrapper.innerHTML); setRenderedIndex(index) }
    }
    load().catch(error => { if (!cancelled) setError(error instanceof Error ? error.message : 'Could not read this page.') }).finally(() => { if (!cancelled) setLoading(false) })
    return () => { cancelled = true; urls.forEach(URL.revokeObjectURL) }
  }, [archive, chapters, index, comic])

  useEffect(() => {
    if (progressReady && renderedIndex === index && !loading && !error && chapters[index]) progress.current?.save({ position: index, total: chapters.length, locator: chapters[index].path })
  }, [index, renderedIndex, chapters, progressReady, loading, error])

  const srcDocument = chapterDocument(content, fontSize, theme)
  return <section className="watch-page book-watch" aria-label="Book reading page">
    <main className="watch-main">
      <header className="watch-header"><button className="watch-back" aria-label="Close reader" onClick={onClose}><ArrowLeft size={23}/> Back</button><div><h1 id="reader-title">{displayTitle(file)}</h1><span>{sourceName} · {comic ? 'CBZ' : 'EPUB'}</span></div></header>
      <div className="reader-toolbar"><button disabled={index === 0 || loading} onClick={() => setIndex(index - 1)} aria-label="Previous"><ChevronLeft size={18}/></button><label>{comic ? 'Page' : 'Chapter'} <select aria-label={comic ? 'Page' : 'Chapter'} value={index} disabled={!chapters.length || loading} onChange={event => setIndex(Number(event.target.value))}>{chapters.map((chapter, i) => <option key={chapter.path} value={i}>{comic ? i + 1 : chapter.label}</option>)}</select> / {chapters.length}</label><button disabled={index >= chapters.length - 1 || loading} onClick={() => setIndex(index + 1)} aria-label="Next"><ChevronRight size={18}/></button>{comic && <label>Page width <input aria-label="Page width" type="range" min="50" max="200" step="10" value={zoom} onChange={event => setZoom(Number(event.target.value))}/>{zoom}%</label>}{!comic && <><label>Text size <input aria-label="Text size" type="range" min="14" max="30" value={fontSize} onChange={event => setFontSize(Number(event.target.value))}/></label><label>Theme <select aria-label="Reading theme" value={theme} onChange={event => setTheme(event.target.value === 'night' ? 'night' : 'paper')}><option value="paper">Paper</option><option value="night">Night</option></select></label></>}</div>
      {loading && <p className="source-message" role="status">Opening book content…</p>}
      {progressError && <p className="video-error" role="status">{progressError}</p>}
      {error && <p className="video-error" role="alert">{error}</p>}
      {!loading && !error && (comic ? <div className="comic-page" key={index}><img style={{ width: `${zoom}%`, maxWidth: 'none' }} src={image} alt={`Page ${index + 1}`} onLoad={() => setRenderedIndex(index)} onError={() => setError('This page image is damaged or unsupported by your browser.')}/></div> : <iframe key={index} title={`Book chapter ${index + 1}`} className="epub-page" sandbox="" referrerPolicy="no-referrer" srcDoc={srcDocument}/>)}
      <MetadataCredits credits={file.metadata?.credits ?? []}/>
    </main>
    <WatchSidebar sourceId={sourceId} path={file.relative_path}/>
  </section>
}
