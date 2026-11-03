import { useEffect, useRef, useState } from 'react'
import {usePlayerPreferences} from './playerPreferences'
import type JASSUB from 'jassub'
import {assWindow} from './assWindow'

type Track = { path: string; label: string; format: 'ass' | 'vtt' }
type Listing = { tracks: Track[]; fonts: number[]; truncated: boolean; warning: string | null }
type Props = { video: HTMLVideoElement | null; sourceId: string; path: string }
export default function VideoSubtitles({ video, sourceId, path }: Props) {
  const preferences=usePlayerPreferences()
  const [listing, setListing] = useState<Listing | null>(null)
  const [selected, setSelected] = useState('')
  const [attempt,setAttempt]=useState(0)
  const fontCache=useRef<{key:string;fonts:Uint8Array[]}|null>(null)
  const [message, setMessage] = useState('')
  const renderer = useRef<JASSUB | null>(null)
  const prepared = useRef<{key:string;text:string;fonts:Uint8Array[];complete:boolean}|null>(null)
  const base = `/api/sources/${encodeURIComponent(sourceId)}`
  useEffect(() => {
    const controller = new AbortController()
    setListing(null); setSelected(''); setMessage(''); prepared.current=null; fontCache.current=null
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
    let removeWindowListeners = () => {}
    setMessage(track.format==='ass'?'Loading subtitle script and embedded fonts…':'Loading subtitles…')
    const load = async () => {
      const key=`${base}:${path}:${selected}`
      const cached=prepared.current?.key===key?prepared.current:null
      const progressive=track.format==='ass'&&selected.startsWith('embedded:')
      const modulePromise=track.format==='ass'?import('jassub'):null
      const script=async()=>{
        if(cached)return cached.text
        const response=await fetch(`${base}/subtitle?${new URLSearchParams({video:path,path:selected,...(progressive?{preview:'true'}:{})})}`,{signal:controller.signal})
        if(!response.ok){const data=await response.json().catch(()=>null) as {error?:string}|null;throw Error(data?.error??'This subtitle track could not be loaded. Try selecting it again.')}
        return response.text()
      }
      const attachments=async()=>{
        if(fontCache.current?.key===`${base}:${path}`)return fontCache.current.fonts
        if(cached)return cached.fonts
        if(track.format!=='ass')return []
        const fonts:Uint8Array[]=new Array(listing.fonts.length)
        let size=0,cursor=0,completed=0
        const worker=async()=>{
          while(cursor<listing.fonts.length){
            const slot=cursor++,index=listing.fonts[slot]
            const font=await fetch(`${base}/subtitle-font?${new URLSearchParams({video:path,index:String(index)})}`,{signal:controller.signal})
            if(!font.ok)throw Error('An embedded subtitle font could not be loaded. Rendering was stopped to avoid incorrect typography.')
            const bytes=new Uint8Array(await font.arrayBuffer())
            size+=bytes.length
            if(size>32*1024*1024)throw Error('Embedded fonts exceed the 32 MiB rendering limit.')
            fonts[slot]=bytes;completed++
            if(!controller.signal.aborted)setMessage(`Loading embedded fonts (${completed}/${listing.fonts.length}) and subtitle script…`)
          }
        }
        await Promise.all([worker(),worker()]);if(!controller.signal.aborted)fontCache.current={key:`${base}:${path}`,fonts};return fonts
      }
      const [text,fonts,module]=await Promise.all([script(),attachments(),modulePromise])
      if(controller.signal.aborted)return
      prepared.current={key,text,fonts,complete:cached?.complete??!progressive}
      if (track.format === 'ass' && module) {
        const Renderer=module.default
        let windows=assWindow(text)
        setMessage('Starting ASS renderer…')
        instance = new Renderer({ video, subContent: windows.at(video.currentTime), fonts, timeOffset: 0, queryFonts: false, libassMemoryLimit: 64, libassGlyphLimit: 10000 })
        // Capture our overlay: a retried video may be detached before destroy(),
        // when the library can no longer locate its original parent.
        const sibling = video.nextElementSibling
        overlay = sibling?.classList.contains('JASSUB') ? sibling : null
        renderer.current = instance
        let timer: ReturnType<typeof setTimeout> | undefined
        try { await Promise.race([instance.ready, new Promise<never>((_, reject) => { timer = setTimeout(() => reject(new Error('Subtitle renderer startup timed out. Check browser WebAssembly/worker support.')), 60000) })]) } finally { clearTimeout(timer) }
        if (controller.signal.aborted) return
        let bucket=windows.key(video.currentTime),revision=0,chain=Promise.resolve()
        let complete=prepared.current.complete
        const refresh=()=>{
          if(!complete&&video.currentTime>=120)setMessage('Subtitles for this position are still being extracted…')
          const next=windows.key(video.currentTime)
          if(next===bucket)return
          bucket=next;const current=++revision,time=video.currentTime
          chain=chain.then(async()=>{
            if(controller.signal.aborted||current!==revision||!instance)return
            await instance.renderer.setTrack(windows.at(time))
            if(!controller.signal.aborted)await instance.resize(true)
          }).catch(()=>{if(!controller.signal.aborted)setMessage('Subtitle window could not update. Select the track again to retry.')})
        }
        video.addEventListener('timeupdate',refresh);video.addEventListener('seeking',refresh)
        removeWindowListeners=()=>{video.removeEventListener('timeupdate',refresh);video.removeEventListener('seeking',refresh)}
        if(!complete){
          setMessage('Subtitles ready for the opening two minutes. Finishing the complete track in the background…')
          void (async()=>{
            const response=await fetch(`${base}/subtitle?${new URLSearchParams({video:path,path:selected})}`,{signal:controller.signal})
            if(!response.ok){const data=await response.json().catch(()=>null) as {error?:string}|null;throw Error(data?.error??'The complete subtitle track could not be extracted. Select the track to retry.')}
            const full=await response.text()
            if(controller.signal.aborted)return
            prepared.current={key,text:full,fonts,complete:true};windows=assWindow(full);complete=true;bucket=-1;refresh();setMessage('')
          })().catch(error=>{if(!controller.signal.aborted)setMessage(error.message)})
        }

      } else {
        objectUrl = URL.createObjectURL(new Blob([text], { type: 'text/vtt' }))
        element = document.createElement('track')
        element.kind = 'subtitles'; element.label = track.label; element.src = objectUrl; element.default = true
        element.addEventListener('error', () => setMessage('Your browser could not load this subtitle track.'), { signal: controller.signal })
        video.appendChild(element)
        element.track.mode = 'showing'
      }
      if(track.format!=='ass'||prepared.current?.complete)setMessage('')
    }
    void load().catch(error => { if (!controller.signal.aborted) { setMessage(error.message); dispose(); renderer.current = null; controller.abort() } })
    return () => {
      removeWindowListeners()
      controller.abort()
      if (renderer.current === instance) renderer.current = null
      dispose()
      element?.remove()
      if (objectUrl) URL.revokeObjectURL(objectUrl)
    }
  }, [video, selected, listing, base, path, attempt])
  const choose=(value:string)=>{if(value&&value===selected)setAttempt(old=>old+1);setSelected(value);const track=listing?.tracks.find(t=>t.path===value);preferences?.update({subtitle:track?{label:track.label,format:track.format,sourceId,path,track:track.path}:null})}
  return <div className="video-subtitles">
    <div className="track-options" role="group" aria-label="Subtitle tracks"><button aria-pressed={!selected} onClick={()=>choose('')}>Off</button>{listing?.tracks.map(track=><button key={track.path} aria-pressed={selected===track.path} onClick={()=>choose(track.path)}>{track.label}{listing.tracks.filter(t=>t.label===track.label).length>1?` · ${track.path.replace('embedded:', 'Stream ')}`:''}</button>)}</div>
    {(message || listing?.warning || listing?.truncated) && <p role="status">{message || listing?.warning || 'Only the first 32 subtitle tracks are shown.'}</p>}
  </div>
}
