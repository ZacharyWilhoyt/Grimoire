import { useFullscreen } from './useFullscreen'
import { useMediaProgress } from './useMediaProgress'
import { displayTitle, type MetadataView } from './media'
import { useCallback, useRef, useState } from 'react'
import MetadataCredits from './MetadataCredits'
import WatchSidebar from './WatchSidebar'
import VideoAudio from './VideoAudio'
import VideoSubtitles from './VideoSubtitles'
import { ArrowLeft, Maximize, Minimize } from 'lucide-react'

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
  const [failed, setFailed] = useState(false)
  const closeButton = useRef<HTMLButtonElement>(null)
  const url = audioUrl ?? `/api/sources/${encodeURIComponent(sourceId)}/video?${new URLSearchParams({ path: file.relative_path })}`

  const { fullscreen, toggle, error: fullscreenError } = useFullscreen(stage)

  return <section className="watch-page" aria-label="Video watch page">
    <main className="watch-main">
      <header className="watch-header"><button ref={closeButton} className="watch-back" aria-label="Close video player" onClick={onClose}><ArrowLeft size={23}/> Back</button><div><h1 id="video-title">{displayTitle(file)}</h1><span>{sourceName}</span></div></header>
      <div className="video-stage watch-stage" ref={stage}>
        <video key={`${url}:${retryKey}`} ref={attachVideo} src={url} controls controlsList="nofullscreen" playsInline preload="metadata" onLoadedData={() => setFailed(false)} onError={() => setFailed(true)} />
        <button className="watch-fullscreen" aria-label={fullscreen ? 'Exit fullscreen' : 'Fullscreen with subtitles'} title={fullscreen ? 'Exit fullscreen' : 'Fullscreen with subtitles'} onClick={() => { void toggle() }}>{fullscreen ? <Minimize size={22}/> : <Maximize size={22}/>}</button>
      </div>
      <div className="watch-settings"><VideoAudio sourceId={sourceId} path={file.relative_path} video={videoElement} onSelect={setAudioUrl}/><VideoSubtitles video={videoElement} sourceId={sourceId} path={file.relative_path}/></div>
      {fullscreenError && <p className="video-error" role="status">{fullscreenError}</p>}
      {progressError && <p className="video-error" role="status">{progressError}</p>}
      {failed && <p className="video-error" role="alert">This video could not be played. Check that the source is available and the video codec is supported. <button className="track-play" onClick={() => { setFailed(false); retry() }}>Retry video</button></p>}
      <MetadataCredits credits={file.metadata?.credits ?? []}/>
    </main>
    <WatchSidebar sourceId={sourceId} path={file.relative_path}/>
  </section>
}
