import { useState } from 'react'
import { Headphones, SkipBack, SkipForward, X } from 'lucide-react'

type Track = { name: string; relative_path: string; source_id: string; source_name: string }
type Props = {
  tracks: Track[]
  busy: boolean
  index: number
  onIndex: (index: number) => void
  onClose: () => void
}

export default function MusicPlayer({ tracks, index, busy, onIndex, onClose }: Props) {
  const [failedTrack, setFailedTrack] = useState<string | null>(null)
  const track = tracks[index]
  const url = `/api/sources/${encodeURIComponent(track.source_id)}/audio?${new URLSearchParams({ path: track.relative_path })}`
  return <section className="music-player" aria-label="Music player">
    <Headphones className="player-icon" size={23} />
    <div className="playing-track"><strong title={track.name}>{track.name}</strong><span>{track.source_name} · {index + 1} of {tracks.length}</span>{failedTrack === url && <span role="alert" className="player-error">Could not play this file. Check the source and browser audio support.</span>}</div>
    <div className="queue-controls"><button aria-label="Previous track" disabled={busy || index === 0} onClick={() => onIndex(index - 1)}><SkipBack size={18}/></button><button aria-label="Next track" disabled={busy || index === tracks.length - 1} onClick={() => onIndex(index + 1)}><SkipForward size={18}/></button></div>
    <audio key={url} src={url} controls autoPlay preload="metadata" onError={() => setFailedTrack(url)} onEnded={() => { if (index + 1 < tracks.length) onIndex(index + 1) }} />
    <button className="close-player" aria-label="Close player" onClick={onClose}><X size={18}/></button>
  </section>
}
