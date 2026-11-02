import { useEffect, useRef, useState } from 'react'
import type JASSUB from 'jassub'

type Track = { path: string; label: string; format: 'ass' | 'vtt' }
type Listing = { tracks: Track[]; fonts: number[]; truncated: boolean; warning: string | null }
type Props = { video: HTMLVideoElement | null; sourceId: string; path: string }
export default function VideoSubtitles({ video, sourceId, path }: Props) {
  const [listing, setListing] = useState<Listing | null>(null)
  const [selected, setSelected] = useState('')
  const [message, setMessage] = useState('')
  const [offset, setOffset] = useState(0)
  const renderer = useRef<JASSUB | null>(null)
  const offsetRef = useRef(0)
  const base = `/api/sources/${encodeURIComponent(sourceId)}`
  useEffect(() => {
    const controller = new AbortController()
    setListing(null); setSelected(''); setMessage(''); setOffset(0)
    fetch(`${base}/subtitles?${new URLSearchParams({ path })}`, { signal: controller.signal })
      .then(async response => { if (!response.ok) throw new Error('Subtitle tracks could not be loaded.'); return await response.json() as Listing })
      .then(setListing).catch(error => { if (!controller.signal.aborted) setMessage(String(error.message)) })
    return () => controller.abort()
  }, [base, path])
  useEffect(() => {
    if (!video || !selected || !listing) return
    const track = listing.tracks.find(track => track.path === selected)
    if (!track) return
    const controller = new AbortController()
    let instance: JASSUB | null = null
    let element: HTMLTrackElement | null = null
    let overlay: Element | null = null
    let destroyed = false
    const dispose = () => { if (instance && !destroyed) { destroyed = true; void instance.destroy().catch(() => {}); overlay?.remove() } }
    let objectUrl: string | null = null
    setMessage('Loading subtitles…')
    const load = async () => {
      const response = await fetch(`${base}/subtitle?${new URLSearchParams({ video: path, path: selected })}`, { signal: controller.signal })
      if (!response.ok) throw new Error('This subtitle track could not be loaded or exceeds processing limits.')
      const text = await response.text()
      if (controller.signal.aborted) return
      if (track.format === 'ass') {
        const { default: Renderer } = await import('jassub')
        const fonts: Uint8Array[] = []
        // Sequential fetching bounds server work and total attachment memory.
        let size = 0
        for (const index of listing.fonts) {
          const font = await fetch(`${base}/subtitle-font?${new URLSearchParams({ video: path, index: String(index) })}`, { signal: controller.signal })
          if (!font.ok) throw new Error('An embedded subtitle font could not be loaded. Rendering was stopped to avoid incorrect typography.')
          const bytes = new Uint8Array(await font.arrayBuffer())
          size += bytes.length
          if (size > 32 * 1024 * 1024) throw new Error('Embedded fonts exceed the 32 MiB rendering limit.')
          fonts.push(bytes)
        }
        if (controller.signal.aborted) return
        instance = new Renderer({ video, subContent: text, fonts, timeOffset: -offsetRef.current, queryFonts: false, libassMemoryLimit: 64, libassGlyphLimit: 10000 })
        // Capture our overlay: a retried video may be detached before destroy(),
        // when the library can no longer locate its original parent.
        const sibling = video.nextElementSibling
        overlay = sibling?.classList.contains('JASSUB') ? sibling : null
        renderer.current = instance
        let timer: ReturnType<typeof setTimeout> | undefined
        try { await Promise.race([instance.ready, new Promise<never>((_, reject) => { timer = setTimeout(() => reject(new Error('Subtitle renderer startup timed out. Check browser WebAssembly/worker support.')), 60000) })]) } finally { clearTimeout(timer) }
        if (controller.signal.aborted) return
      } else {
        objectUrl = URL.createObjectURL(new Blob([text], { type: 'text/vtt' }))
        element = document.createElement('track')
        element.kind = 'subtitles'; element.label = track.label; element.src = objectUrl; element.default = true
        element.addEventListener('error', () => setMessage('Your browser could not load this subtitle track.'), { signal: controller.signal })
        video.appendChild(element)
        element.track.mode = 'showing'
      }
      setMessage('')
    }
    void load().catch(error => { if (!controller.signal.aborted) { setMessage(error.message); dispose(); renderer.current = null } })
    return () => {
      controller.abort()
      if (renderer.current === instance) renderer.current = null
      dispose()
      element?.remove()
      if (objectUrl) URL.revokeObjectURL(objectUrl)
    }
  }, [video, selected, listing, base, path])
  useEffect(() => {
    offsetRef.current = offset
    const instance = renderer.current
    if (instance) {
      instance.timeOffset = -offset
      void instance.ready.then(() => { if (renderer.current === instance) return instance.resize(true) }).catch(() => {})
    }
  }, [offset])
  const ass = listing?.tracks.find(track => track.path === selected)?.format === 'ass'
  return <div className="video-subtitles">
    <label>Subtitles <select aria-label="Subtitle track" value={selected} onChange={event => { setSelected(event.target.value); setOffset(0) }}><option value="">Off</option>{listing?.tracks.map(track => <option key={track.path} value={track.path}>{track.label}</option>)}</select></label>
    {ass && <p>ASS uses libass with embedded fonts. Fonts missing from the file use the bundled fallback; online font lookup is disabled. <a href="/licenses/subtitle-renderer.txt" target="_blank" rel="noreferrer">Renderer credits</a>.</p>}
    {ass && <label>Subtitle delay (seconds) <input aria-label="Subtitle delay in seconds" type="number" min={-60} max={60} step={0.1} value={offset} onChange={event => { const value = Number(event.target.value); if (Number.isFinite(value)) setOffset(Math.max(-60, Math.min(60, value))) }}/></label>}
    {(message || listing?.warning || listing?.truncated) && <p role="status">{message || listing?.warning || 'Only the first 32 subtitle tracks are shown.'}</p>}
  </div>
}
