import { adjacentTrack, playOrder, type RepeatMode } from './musicQueue'
import { useMediaProgress } from './useMediaProgress'
import { displayTitle, type MetadataView } from './media'
import { useMemo, useState } from 'react'
import { Headphones, SkipBack, SkipForward, Shuffle, Repeat, Repeat1, X } from 'lucide-react'

type Track = { metadata?: MetadataView | null; name: string; relative_path: string; source_id: string; source_name: string }
type Props = {
  tracks: Track[]
  busy: boolean
  index: number
  onIndex: (index: number) => void
  onClose: () => void
}

export default function MusicPlayer({ tracks, index, busy, onIndex, onClose }: Props) {
  const [failedTrack, setFailedTrack] = useState<string | null>(null)
  const [shuffle, setShuffle] = useState(false)
  const [repeat, setRepeat] = useState<RepeatMode>('off')
  const queueKey = JSON.stringify(tracks.map(track => [track.source_id, track.relative_path]))
  // Keep one shuffle permutation across track changes; toggling or replacing the
  // queue starts a new order at the currently selected track.
  const order = useMemo(() => playOrder(tracks.length, index, shuffle), [queueKey, shuffle])
  const previous = adjacentTrack(order, index, -1, repeat)
  const next = adjacentTrack(order, index, 1, repeat)
  const track = tracks[index]
  const { media, progressError, retry, retryKey } = useMediaProgress<HTMLAudioElement>(track.source_id, track.relative_path, '', false)
  const url = `/api/sources/${encodeURIComponent(track.source_id)}/audio?${new URLSearchParams({ path: track.relative_path })}`
  const select = (target: number | undefined) => {
    if (target === undefined) return
    if (target === index && media.current) { media.current.currentTime = 0; void media.current.play().catch(() => {}) }
    else onIndex(target)
  }
  return <section className="music-player" aria-label="Music player">
    <Headphones className="player-icon" size={23} />
    <div className="playing-track"><strong title={displayTitle(track)}>{displayTitle(track)}</strong><span>{track.source_name} · {index + 1} of {tracks.length}</span>{failedTrack === url && <span role="alert" className="player-error">Could not play this file. Check the source and browser audio support. <button className="track-play" disabled={busy} onClick={() => { setFailedTrack(null); retry() }}>Retry audio</button></span>}</div>
    <div className="queue-controls"><button aria-label="Previous track" disabled={busy || previous === undefined} onClick={() => select(previous)}><SkipBack size={18}/></button><button aria-label="Next track" disabled={busy || next === undefined} onClick={() => select(next)}><SkipForward size={18}/></button><button aria-label="Shuffle" title="Shuffle queue" aria-pressed={shuffle} disabled={busy || tracks.length < 2} onClick={() => setShuffle(value => !value)}><Shuffle size={17}/></button><button aria-label={`Repeat: ${repeat}`} title={`Repeat: ${repeat}. Click to cycle off, all, one.`} aria-pressed={repeat !== 'off'} disabled={busy} onClick={() => setRepeat(value => value === 'off' ? 'all' : value === 'all' ? 'one' : 'off')}>{repeat === 'one' ? <Repeat1 size={17}/> : <Repeat size={17}/>}</button></div>
    <audio ref={media} key={`${url}:${retryKey}`} src={url} controls loop={repeat === 'one' || repeat === 'all' && tracks.length === 1} preload="metadata" onLoadedData={() => setFailedTrack(null)} onError={() => setFailedTrack(url)} onEnded={() => select(next)} />
    {progressError && <span className="player-error" role="status">{progressError}</span>}
    <button className="close-player" aria-label="Close player" onClick={onClose}><X size={18}/></button>
  </section>
}
