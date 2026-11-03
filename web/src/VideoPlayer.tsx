import { useFullscreen } from './useFullscreen'
import { useMediaProgress } from './useMediaProgress'
import { displayTitle, type MetadataView } from './media'
import { useCallback, useEffect, useRef, useState } from 'react'
import { apiFetch } from './api'
import MetadataCredits from './MetadataCredits'
import WatchSidebar from './WatchSidebar'
import VideoToolbar from './VideoToolbar'
import { ArrowLeft } from 'lucide-react'

type Props = {
  sourceId: string
  sourceName: string
  file: { metadata?: MetadataView | null; name: string; relative_path: string }
  onClose: () => void
}

export default function VideoPlayer({ sourceId, sourceName, file, onClose }: Props) {
  const [audioUrl, setAudioUrl] = useState<string | null>(null)
  const { media, progressError, retry, retryKey } = useMediaProgress<HTMLVideoElement>(sourceId, file.relative_path, audioUrl ?? '')
  const [videoElement, setVideoElement] = useState<HTMLVideoElement | null>(null)
  const attachVideo = useCallback((element: HTMLVideoElement | null) => { media.current = element; setVideoElement(element) }, [media])
  const stage = useRef<HTMLDivElement>(null)
  const [cozy, setCozy] = useState(false)
  const [failed, setFailed] = useState(false)
  const closeButton = useRef<HTMLButtonElement>(null)
  const url = audioUrl ?? `/api/sources/${encodeURIComponent(sourceId)}/video?${new URLSearchParams({ path: file.relative_path })}`

  const [streamError,setStreamError]=useState('')
  const segmented = url.includes('/manifest.m3u8')
  useEffect(()=>{
    setStreamError('')
    if(!segmented||!videoElement)return
    let active=true,dispose:(()=>void)|undefined
    void import('hls.js').then(({default:Hls})=>{
      if(!active)return
      if(Hls.isSupported()){
        const hls=new Hls({maxBufferLength:12,maxMaxBufferLength:24,backBufferLength:12,fragLoadingTimeOut:100000})
        hls.on(Hls.Events.ERROR,(_event,data)=>{if(data.fatal&&active)setStreamError('Segmented playback failed. Try CPU encoding, a lower resolution, or restart transcoding. Access may have expired.')})
        hls.loadSource(url);hls.attachMedia(videoElement);dispose=()=>hls.destroy()
      }else if(videoElement.canPlayType('application/vnd.apple.mpegurl'))videoElement.src=url
      else setStreamError('This browser does not support HLS playback.')
    }).catch(()=>{if(active)setStreamError('The streaming player could not load.')})
    return()=>{active=false;dispose?.()}
  },[url,videoElement,segmented])
  useEffect(()=>{if(!segmented)return;const id=url.split('/')[3];return()=>{void apiFetch(`/api/transcode/${encodeURIComponent(id)}`,{method:'DELETE',keepalive:true}).catch(()=>{})}},[url,segmented])
  const { fullscreen, toggle, error: fullscreenError } = useFullscreen(stage)

  return <section className={`watch-page ${cozy ? 'watch-cozy' : ''}`} aria-label="Video watch page">
    <main className="watch-main">
      <header className="watch-header"><button ref={closeButton} className="watch-back" aria-label="Close video player" onClick={onClose}><ArrowLeft size={23}/> Back</button><div><h1 id="video-title">{displayTitle(file)}</h1><span>{sourceName}</span></div></header>
      <div className="video-stage watch-stage" ref={stage}>
        <video key={`${url}:${retryKey}`} ref={attachVideo} src={segmented ? undefined : url} playsInline preload="metadata" onLoadedData={() => setFailed(false)} onError={() => setFailed(true)} />
        <VideoToolbar video={videoElement} sourceId={sourceId} path={file.relative_path} onAudio={setAudioUrl} cozy={cozy} onCozy={() => setCozy(value=>!value)} fullscreen={fullscreen} onFullscreen={() => { void toggle() }}/>
      </div>
      {streamError && <p role="alert" className="video-error">{streamError}</p>}
      {fullscreenError && <p className="video-error" role="status">{fullscreenError}</p>}
      {progressError && <p className="video-error" role="status">{progressError}</p>}
      {failed && <p className="video-error" role="alert">This video could not be played. Check that the source is available and the video codec is supported. <button className="track-play" onClick={() => { setFailed(false); retry() }}>Retry video</button></p>}
      <MetadataCredits credits={file.metadata?.credits ?? []}/>
    </main>
    {!cozy && <WatchSidebar sourceId={sourceId} path={file.relative_path}/>}
  </section>
}
