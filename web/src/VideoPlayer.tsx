import { useFullscreen } from './useFullscreen'
import { useMediaProgress } from './useMediaProgress'
import { displayTitle, type MetadataView } from './media'
import { useCallback, useRef, useState } from 'react'
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

  const { fullscreen, toggle, error: fullscreenError } = useFullscreen(stage)

  return <section className={`watch-page ${cozy ? 'watch-cozy' : ''}`} aria-label="Video watch page">
    <main className="watch-main">
      <header className="watch-header"><button ref={closeButton} className="watch-back" aria-label="Close video player" onClick={onClose}><ArrowLeft size={23}/> Back</button><div><h1 id="video-title">{displayTitle(file)}</h1><span>{sourceName}</span></div></header>
      <div className="video-stage watch-stage" ref={stage}>
        <video key={`${url}:${retryKey}`} ref={attachVideo} src={url} playsInline preload="metadata" onLoadedData={() => setFailed(false)} onError={() => setFailed(true)} />
        <VideoToolbar video={videoElement} sourceId={sourceId} path={file.relative_path} onAudio={setAudioUrl} cozy={cozy} onCozy={() => setCozy(value=>!value)} fullscreen={fullscreen} onFullscreen={() => { void toggle() }}/>
      </div>
      {fullscreenError && <p className="video-error" role="status">{fullscreenError}</p>}
      {progressError && <p className="video-error" role="status">{progressError}</p>}
      {failed && <p className="video-error" role="alert">This video could not be played. Check that the source is available and the video codec is supported. <button className="track-play" onClick={() => { setFailed(false); retry() }}>Retry video</button></p>}
      <MetadataCredits credits={file.metadata?.credits ?? []}/>
    </main>
    {!cozy && <WatchSidebar sourceId={sourceId} path={file.relative_path}/>}
  </section>
}
