import { useMediaProgress } from './useMediaProgress'
import { displayTitle, type MetadataView } from './media'
import { useCallback, useEffect, useRef, useState } from 'react'
import VideoSubtitles from './VideoSubtitles'
import { X } from 'lucide-react'

type Props = {
  sourceId: string
  sourceName: string
  file: { metadata?: MetadataView | null; name: string; relative_path: string }
  onClose: () => void
}

export default function VideoPlayer({ sourceId, sourceName, file, onClose }: Props) {
  const { media, progressError, retry, retryKey } = useMediaProgress<HTMLVideoElement>(sourceId, file.relative_path)
  const [videoElement, setVideoElement] = useState<HTMLVideoElement | null>(null)
  const attachVideo = useCallback((element: HTMLVideoElement | null) => { media.current = element; setVideoElement(element) }, [media])
  const stage = useRef<HTMLDivElement>(null)
  const [failed, setFailed] = useState(false)
  const dialog = useRef<HTMLDialogElement>(null)
  const closeButton = useRef<HTMLButtonElement>(null)
  const url = `/api/sources/${encodeURIComponent(sourceId)}/video?${new URLSearchParams({ path: file.relative_path })}`

  useEffect(() => {
    const element = dialog.current
    element?.showModal()
    closeButton.current?.focus()
    return () => element?.close()
  }, [])

  return <dialog ref={dialog} className="video-backdrop" aria-labelledby="video-title" onCancel={event => { event.preventDefault(); onClose() }} onClick={event => { if (event.target === event.currentTarget) onClose() }}>
    <section className="video-dialog" aria-labelledby="video-title">
      <header className="video-header"><div><h2 id="video-title">{displayTitle(file)}</h2><span>{sourceName}</span></div><button ref={closeButton} aria-label="Close video player" onClick={onClose}><X size={21}/></button></header>
      <div className="video-stage" ref={stage}><video key={`${url}:${retryKey}`} ref={attachVideo} src={url} controls playsInline preload="metadata" onLoadedData={() => setFailed(false)} onError={() => setFailed(true)} /></div>
      <VideoSubtitles key={url} video={videoElement} sourceId={sourceId} path={file.relative_path}/>
      <button className="video-fullscreen" onClick={() => { void stage.current?.requestFullscreen().catch(() => {}) }}>Fullscreen with subtitles</button>
      {progressError && <p className="video-error" role="status">{progressError}</p>}
      {failed && <p className="video-error" role="alert">This video could not be played. Check that the source is available. Your browser may not support its container or codecs; conversion is not available yet. <button className="track-play" onClick={() => { setFailed(false); retry() }}>Retry video</button></p>}
      <p className="video-hint">Use Fullscreen with subtitles to keep styled subtitles visible. Native video fullscreen and picture-in-picture may omit ASS overlays.</p>
    </section>
  </dialog>
}
