import {mediaPoint,monitorRelayStall,type PlaybackControls} from './playbackRouting'
import { adjacentTrack, playOrder, type RepeatMode } from './musicQueue'
import { useMediaProgress } from './useMediaProgress'
import { displayTitle, type MetadataView } from './media'
import { useEffect, useMemo, useRef, useState } from 'react'
import { Headphones, SkipBack, SkipForward, Shuffle, Repeat, Repeat1, X } from 'lucide-react'

type Track = { logical_id?: string; media_url?: string; metadata?: MetadataView | null; name: string; relative_path: string; source_id: string; source_name: string }
type Props = PlaybackControls & {
  tracks: Track[]
  busy: boolean
  index: number
  onIndex: (index: number) => void
  onClose: () => void
}

export default function MusicPlayer({ tracks, index, busy, onIndex, onClose, sourceControl, resume, onSourceFailure, onCapture }: Props) {
  const [failedTrack, setFailedTrack] = useState<string | null>(null)
  const [shuffle, setShuffle] = useState(false)
  const [repeat, setRepeat] = useState<RepeatMode>('off')
  const queueKey = JSON.stringify(tracks.map(track => track.logical_id ?? [track.source_id, track.relative_path]))
  // Keep one shuffle permutation across track changes; toggling or replacing the
  // queue starts a new order at the currently selected track.
  const order = useMemo(() => playOrder(tracks.length, index, shuffle), [queueKey, shuffle])
  const previous = adjacentTrack(order, index, -1, repeat)
  const next = adjacentTrack(order, index, 1, repeat)
  const track = tracks[index]
  const { media, progressError, retry, retryKey } = useMediaProgress<HTMLAudioElement>(track.source_id, track.relative_path, track.media_url ?? '', false, resume)
  const url = track.media_url ?? `/api/sources/${encodeURIComponent(track.source_id)}/audio?${new URLSearchParams({ path: track.relative_path })}`
  const reported=useRef<string|null>(null)
  useEffect(()=>{onCapture?.(()=>mediaPoint(media.current));return()=>onCapture?.(null)},[media,onCapture])
  const failure=useRef(onSourceFailure)
  failure.current=onSourceFailure
  const report=()=>{
    setFailedTrack(url)
    if(reported.current!==url){reported.current=url;failure.current?.(mediaPoint(media.current))}
  }
  useEffect(()=>{
    if(!track.media_url||!media.current)return
    return monitorRelayStall(media.current,report)
  },[track.media_url,retryKey])
  const select = (target: number | undefined) => {
    if (target === undefined) return
    if (target === index && media.current) { media.current.currentTime = 0; void media.current.play().catch(() => {}) }
    else onIndex(target)
  }
  return <section className="music-player" aria-label="Music player">
    <Headphones className="player-icon" size={23} />
    <div className="playing-track"><strong title={displayTitle(track)}>{displayTitle(track)}</strong><span>{track.source_name} · {index + 1} of {tracks.length}</span>{failedTrack === url && <span role="alert" className="player-error">Could not play this file. Check the source and browser audio support. <button className="track-play" disabled={busy} onClick={() => { setFailedTrack(null); reported.current=null; retry() }}>Retry audio</button></span>}</div>
    <div className="queue-controls"><button aria-label="Previous track" disabled={busy || previous === undefined} onClick={() => select(previous)}><SkipBack size={18}/></button><button aria-label="Next track" disabled={busy || next === undefined} onClick={() => select(next)}><SkipForward size={18}/></button><button aria-label="Shuffle" title="Shuffle queue" aria-pressed={shuffle} disabled={busy || tracks.length < 2} onClick={() => setShuffle(value => !value)}><Shuffle size={17}/></button><button aria-label={`Repeat: ${repeat}`} title={`Repeat: ${repeat}. Click to cycle off, all, one.`} aria-pressed={repeat !== 'off'} disabled={busy} onClick={() => setRepeat(value => value === 'off' ? 'all' : value === 'all' ? 'one' : 'off')}>{repeat === 'one' ? <Repeat1 size={17}/> : <Repeat size={17}/>}</button></div>
    <audio ref={media} key={`${url}:${retryKey}`} src={url} controls loop={repeat === 'one' || repeat === 'all' && tracks.length === 1} preload="metadata" onLoadedData={() => setFailedTrack(null)} onError={report} onEnded={() => select(next)} />
    {sourceControl}
    {progressError && <span className="player-error" role="status">{progressError}</span>}
    <button className="close-player" aria-label="Close player" onClick={onClose}><X size={18}/></button>
  </section>
}
