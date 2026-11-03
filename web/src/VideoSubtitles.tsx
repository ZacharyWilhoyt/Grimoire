import { useEffect, useRef, useState } from 'react'
import {usePlayerPreferences} from './playerPreferences'
import type JASSUB from 'jassub'

type Track = { path: string; label: string; format: 'ass' | 'vtt' }
type Listing = { tracks: Track[]; fonts: number[]; truncated: boolean; warning: string | null }
type Props = { video: HTMLVideoElement | null; sourceId: string; path: string }
export default function VideoSubtitles({ video, sourceId, path }: Props) {
  const preferences=usePlayerPreferences()
  const [listing, setListing] = useState<Listing | null>(null)
  const [selected, setSelected] = useState('')
  const [message, setMessage] = useState('')
  const renderer = useRef<JASSUB | null>(null)
  const base = `/api/sources/${encodeURIComponent(sourceId)}`
  useEffect(() => {
    const controller = new AbortController()
    setListing(null); setSelected(''); setMessage('')
    fetch(`${base}/subtitles?${new URLSearchParams({ path })}`, { signal: controller.signal })
      .then(async response => { if (!response.ok) throw new Error('Subtitle tracks could not be loaded.'); return await response.json() as Listing })
      .then(data=>{if(controller.signal.aborted)return;setListing(data);const saved=preferences?.settings.subtitle;if(saved){const track=(saved.sourceId===sourceId&&saved.path===path?data.tracks.find(t=>t.path===saved.track&&t.label===saved.label&&t.format===saved.format):undefined)??data.tracks.find(t=>t.label===saved.label&&t.format===saved.format);if(track)setSelected(track.path)}}).catch(error => { if (!controller.signal.aborted) setMessage(String(error.message)) })
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
      if (!response.ok) { const data = await response.json().catch(() => null) as { error?: string } | null; throw new Error(data?.error ?? 'This subtitle track could not be loaded. Try selecting it again.') }
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
        instance = new Renderer({ video, subContent: text, fonts, timeOffset: 0, queryFonts: false, libassMemoryLimit: 64, libassGlyphLimit: 10000 })
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
  const choose=(value:string)=>{setSelected(value);const track=listing?.tracks.find(t=>t.path===value);preferences?.update({subtitle:track?{label:track.label,format:track.format,sourceId,path,track:track.path}:null})}
  return <div className="video-subtitles">
    <div className="track-options" role="group" aria-label="Subtitle tracks"><button aria-pressed={!selected} onClick={()=>choose('')}>Off</button>{listing?.tracks.map(track=><button key={track.path} aria-pressed={selected===track.path} onClick={()=>choose(track.path)}>{track.label}{listing.tracks.filter(t=>t.label===track.label).length>1?` · ${track.path.replace('embedded:', 'Stream ')}`:''}</button>)}</div>
    {(message || listing?.warning || listing?.truncated) && <p role="status">{message || listing?.warning || 'Only the first 32 subtitle tracks are shown.'}</p>}
  </div>
}
