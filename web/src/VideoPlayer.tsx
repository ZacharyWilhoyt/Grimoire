import { useEffect, useRef, useState } from 'react'
import { X } from 'lucide-react'

type Props = {
  sourceId: string
  sourceName: string
  file: { name: string; relative_path: string }
  onClose: () => void
}

export default function VideoPlayer({ sourceId, sourceName, file, onClose }: Props) {
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
      <header className="video-header"><div><h2 id="video-title">{file.name}</h2><span>{sourceName}</span></div><button ref={closeButton} aria-label="Close video player" onClick={onClose}><X size={21}/></button></header>
      <video src={url} controls autoPlay playsInline preload="metadata" onError={() => setFailed(true)} />
      {failed && <p className="video-error" role="alert">This video could not be played. Check that the source is available. Your browser may not support its container or codecs; conversion is not available yet.</p>}
      <p className="video-hint">Use the player controls to seek, adjust volume, or enter fullscreen.</p>
    </section>
  </dialog>
}
