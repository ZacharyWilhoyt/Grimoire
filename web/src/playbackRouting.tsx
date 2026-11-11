import {apiFetch} from './api'
import type {MediaItem} from './media'

export type ResumePoint = { position: number; total?: number; locator?: string; exact?: boolean; paused?: boolean; volume?: number; muted?: boolean }
type SourceOption = {id:string; source:string; extension:string; size_bytes:number}
export type PlaybackRoute = {session:string;location:string;item:MediaItem;options:SourceOption[];exact:boolean}
export type PlaybackControls = {
  sourceControl?: React.ReactNode
  resume?: ResumePoint
  onSourceFailure?: (point:ResumePoint) => void
  onCapture?: (read: (() => ResumePoint) | null) => void
}

const formats = [
  ['mp4','video/mp4'], ['m4v','video/mp4'], ['webm','video/webm'], ['mkv','video/x-matroska'],
  ['mp3','audio/mpeg'], ['m4a','audio/mp4'], ['flac','audio/flac'], ['aac','audio/aac'],
  ['ogg','audio/ogg'], ['opus','audio/ogg; codecs=opus'], ['wav','audio/wav'],
]

async function post(path:string, data:unknown, signal?:AbortSignal):Promise<PlaybackRoute> {
  const response=await apiFetch(path,{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify(data),signal})
  const value=await response.json()
  if(!response.ok) throw new Error(value.error??'No compatible playback source is currently available.')
  return value
}

export function resolvePlayback(item:MediaItem,signal?:AbortSignal) {
  const audio=document.createElement('audio'),video=document.createElement('video')
  const native_formats=formats.filter(([,mime])=>(mime.startsWith('audio/')?audio:video).canPlayType(mime)).map(([format])=>format)
  return post('/api/playback/v1',{source_id:item.source_id,relative_path:item.relative_path,native_formats},signal)
}

export function switchPlayback(route:PlaybackRoute,prefer?:string) {
  return post(`/api/playback/v1/${encodeURIComponent(route.session)}/switch`,{current:route.location,prefer})
}

export function mediaPoint(element:HTMLMediaElement|null):ResumePoint {
  return {position:element?.currentTime??0,total:element&&Number.isFinite(element.duration)?element.duration:undefined,paused:element?.paused,volume:element?.volume,muted:element?.muted}
}

export function monitorRelayStall(media:HTMLMediaElement, onStall:()=>void) {
  let timer:ReturnType<typeof setTimeout>|undefined
  const stop=()=>clearTimeout(timer)
  const waiting=()=>{
    stop()
    if(media.paused||media.ended)return
    const position=media.currentTime
    timer=setTimeout(()=>{
      if(!media.paused&&!media.ended&&media.readyState<3&&Math.abs(media.currentTime-position)<.2)onStall()
    },25_000)
  }
  const clearEvents=['playing','pause','ended','emptied']
  media.addEventListener('waiting',waiting)
  clearEvents.forEach(event=>media.addEventListener(event,stop))
  return ()=>{
    stop()
    media.removeEventListener('waiting',waiting)
    clearEvents.forEach(event=>media.removeEventListener(event,stop))
  }
}

export function PlaybackSource({route,busy,message,onSelect}:{route:PlaybackRoute;busy:boolean;message:string;onSelect:(source:string)=>void}) {
  return <div className="playback-source">
    {route.options.length>1&&<label>Playback source <select aria-label="Playback source" value={route.location} disabled={busy} onChange={event=>onSelect(event.target.value)}>
      {route.options.map(option=><option key={option.id} value={option.id}>{option.source} · {option.extension.toUpperCase()} · {Math.round(option.size_bytes/1024**2)} MiB</option>)}
    </select></label>}
    {(busy||message)&&<span role="status">{busy?'Checking playback sources…':message}</span>}
  </div>
}
