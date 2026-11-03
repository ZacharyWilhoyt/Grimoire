import {PlayerPreferences} from './playerPreferences'
import { useFullscreen } from './useFullscreen'
import { useMediaProgress } from './useMediaProgress'
import { displayTitle, type MetadataView } from './media'
import { useCallback, useEffect, useRef, useState } from 'react'
import { apiFetch } from './api'
import MetadataCredits from './MetadataCredits'
import WatchSidebar from './WatchSidebar'
import VideoToolbar from './VideoToolbar'
import type { MediaItem } from './media'
import type { LinkContext } from './mediaLinks'
import { ArrowLeft, Maximize2, X, Play, Pause } from 'lucide-react'

type Props = {
  mini?: boolean
  onRestore?: () => void
  onDismiss?: () => void
  onNext?: (item: MediaItem) => void
  userId?: string
  sourceId: string
  sourceName: string
  file: { metadata?: MetadataView | null; name: string; relative_path: string }
  onClose: () => void
}

export default function VideoPlayer(props:Props){return <PlayerPreferences key={props.userId} userId={props.userId}><Player {...props}/></PlayerPreferences>}
function Player({ sourceId, sourceName, file, onClose, mini, onRestore, onDismiss, onNext }: Props) {
  const [audioUrl, setAudioUrl] = useState<string | null>(null)
  const { media, progressError, retry, retryKey } = useMediaProgress<HTMLVideoElement>(sourceId, file.relative_path, audioUrl ?? '')
  const [videoElement, setVideoElement] = useState<HTMLVideoElement | null>(null)
  const attachVideo = useCallback((element: HTMLVideoElement | null) => { media.current = element; setVideoElement(element) }, [media])
  const stage = useRef<HTMLDivElement>(null)
  const [cozy, setCozy] = useState(false)
  const [failed, setFailed] = useState(false)
  const closeButton = useRef<HTMLButtonElement>(null)
  const url = audioUrl ?? ''

  const [streamError,setStreamError]=useState('')
  const segmented = url.includes('/manifest.m3u8')
  useEffect(()=>{
    setStreamError('')
    if(!segmented||!videoElement)return
    let active=true,dispose:(()=>void)|undefined
    void import('hls.js').then(({default:Hls})=>{
      if(!active)return
      if(Hls.isSupported()){
        const hls=new Hls({maxBufferLength:36,maxMaxBufferLength:60,backBufferLength:12,fragLoadingTimeOut:100000})
        hls.on(Hls.Events.ERROR,(_event,data)=>{if(data.fatal&&active)setStreamError('Segmented playback failed. Try a lower quality, restart playback, or ask the source owner to check its encoder. Access may have expired.')})
        hls.loadSource(url);hls.attachMedia(videoElement);dispose=()=>hls.destroy()
      }else if(videoElement.canPlayType('application/vnd.apple.mpegurl'))videoElement.src=url
      else setStreamError('This browser does not support HLS playback.')
    }).catch(()=>{if(active)setStreamError('The streaming player could not load.')})
    return()=>{active=false;dispose?.()}
  },[url,videoElement,segmented])
  useEffect(()=>{if(!segmented)return;const id=url.split('/')[3];const stop=()=>{void apiFetch(`/api/transcode/${encodeURIComponent(id)}`,{method:'DELETE',keepalive:true}).catch(()=>{})};const leaving=(event:PageTransitionEvent)=>{if(!event.persisted)stop()};window.addEventListener('pagehide',leaving);return()=>{window.removeEventListener('pagehide',leaving);stop()}},[url,segmented])
  const { fullscreen, toggle, error: fullscreenError } = useFullscreen(stage)

  const [next, setNext] = useState<MediaItem | null>(null)
  const [playing, setPlaying] = useState(false), [position, setPosition] = useState(0)
  useEffect(() => {
    const abort = new AbortController()
    void apiFetch(`/api/media/link?${new URLSearchParams({source_id:sourceId,path:file.relative_path})}`, {signal:abort.signal}).then(async r => {
      if (!r.ok) return
      const context = await r.json() as LinkContext
      const index = context.members.findIndex(item => item.relative_path === file.relative_path)
      if (!abort.signal.aborted) setNext(index >= 0 ? context.members[index + 1] ?? null : null)
    }).catch(() => {})
    return () => abort.abort()
  }, [sourceId,file.relative_path])
  useEffect(() => {
    if (!videoElement) return
    const update = () => {setPlaying(!videoElement.paused);setPosition(Number.isFinite(videoElement.duration) && videoElement.duration > 0 ? videoElement.currentTime / videoElement.duration : 0)}
    const ended = () => {if(next) onNext?.(next)}
    videoElement.addEventListener('ended',ended)
    ;['play','pause','timeupdate'].forEach(e=>videoElement.addEventListener(e,update))
    return () => {videoElement.removeEventListener('ended',ended);['play','pause','timeupdate'].forEach(e=>videoElement.removeEventListener(e,update))}
  },[videoElement,next,onNext])
  const [miniVisible, setMiniVisible] = useState(false)
  const miniTimer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined)
  const revealMini = () => {
    setMiniVisible(true); clearTimeout(miniTimer.current)
    miniTimer.current = setTimeout(() => {
      setMiniVisible(false)
    },2500)
  }
  useEffect(() => () => clearTimeout(miniTimer.current),[])
  const minimize = () => {if(document.fullscreenElement) void document.exitFullscreen().catch(()=>{});onClose()}
  return <section className={`watch-page video-watch ${cozy ? 'watch-cozy' : ''} ${mini ? 'video-mini' : ''}`} aria-label="Video watch page">
    <main className="watch-main">
      {!cozy && <header className="watch-header"><button ref={closeButton} className="watch-back" aria-label="Close video player" onClick={minimize}><ArrowLeft size={23}/> Back</button><div><h1 id="video-title">{displayTitle(file)}</h1><span>{sourceName}</span></div></header>}
      <div className="video-stage watch-stage" ref={stage}>
        <video key={`${url}:${retryKey}`} ref={attachVideo} src={undefined} playsInline preload="metadata" onLoadedData={() => setFailed(false)} onError={() => setFailed(true)} />
        <VideoToolbar onNext={next && onNext ? () => onNext(next) : undefined} onClose={minimize} video={videoElement} sourceId={sourceId} path={file.relative_path} onAudio={setAudioUrl} cozy={cozy} onCozy={() => setCozy(value=>!value)} fullscreen={fullscreen} onFullscreen={() => { if(mini)onRestore?.(); void toggle() }}/>
        {mini && <div className={`mini-controls ${miniVisible ? 'mini-visible' : ''}`} onPointerMove={revealMini} onPointerDown={revealMini} onFocus={revealMini} onBlur={revealMini} onPointerLeave={()=>{clearTimeout(miniTimer.current);miniTimer.current=setTimeout(()=>setMiniVisible(false),2500)}}><button className="mini-restore" aria-label="Restore video player" onClick={onRestore}><Maximize2/></button><button className="mini-close" aria-label="Stop video" onClick={onDismiss}><X/></button><button className="mini-play" aria-label={playing?'Pause video':'Play video'} onClick={()=>{if(videoElement?.paused) void videoElement.play().catch(()=>{});else videoElement?.pause()}}>{playing?<Pause/>:<Play/>}</button><progress aria-label="Video progress" max={1} value={position}/></div>}
      </div>
      {!audioUrl && <p className="video-error" role="status">Preparing transcoded playback. Open Settings to retry or change quality.</p>}
      {streamError && <p role="alert" className="video-error">{streamError}</p>}
      {fullscreenError && <p className="video-error" role="status">{fullscreenError}</p>}
      {progressError && <p className="video-error" role="status">{progressError}</p>}
      {failed && <p className="video-error" role="alert">This video could not be played. Check that the source is available and the video codec is supported. <button className="track-play" onClick={() => { setFailed(false); retry() }}>Retry video</button></p>}
      <MetadataCredits credits={file.metadata?.credits ?? []}/>
    </main>
    {!cozy && <WatchSidebar sourceId={sourceId} path={file.relative_path}/>}
  </section>
}
