import { useEffect, useRef, useState } from 'react'
import {usePlayerPreferences} from './playerPreferences'
import { apiFetch } from './api'
type Track = { index: number; label: string; codec: string; default: boolean }
type Props = { sourceId: string; path: string; video: HTMLVideoElement | null; onSelect: (url: string | null) => void; profile?: {encoder:string;height:number}; onTrack?: (index:number|null)=>void }
export default function VideoAudio({ sourceId, path, video, onSelect, profile = {encoder:'auto',height:720}, onTrack }: Props) {
  const preferences=usePlayerPreferences()
  const [tracks, setTracks] = useState<Track[]>([]), [selected, setSelected] = useState(''), [busy, setBusy] = useState(false), [error, setError] = useState('')
  const generation = useRef(0)
  useEffect(() => {
    const controller = new AbortController()
    void apiFetch(`/api/sources/${encodeURIComponent(sourceId)}/audio-tracks?${new URLSearchParams({ path })}`, { signal: controller.signal }).then(async response => {
      if (!response.ok) throw new Error('Audio tracks could not be loaded. Original audio is still available.')
      const data = await response.json() as Track[]
      if (!controller.signal.aborted) setTracks(data)
    }).catch(error => { if (!controller.signal.aborted) setError(error instanceof Error ? error.message : 'Audio tracks could not be loaded.') })
    return () => { controller.abort(); generation.current++ }
  }, [sourceId, path])
  async function select(value: string) {
    const current = ++generation.current
    setError('')
    if (!value) { setSelected(''); onTrack?.(null); preferences?.update({transcode:false}); onSelect(null); return }
    video?.pause(); setBusy(true)
    try {
      const response = await apiFetch('/api/transcode', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ source_id: sourceId, path, ...profile, audio: Number(value) }) })
      const data = await response.json() as { id?: string; error?: string }
      if (!response.ok || !data.id) throw new Error(data.error ?? 'Could not prepare this audio track.')
      if (current !== generation.current) { void apiFetch(`/api/transcode/${encodeURIComponent(data.id)}`,{method:'DELETE'}).catch(()=>{}); return }
      preferences?.update({encoder:profile.encoder,height:profile.height,transcode:true});
      setSelected(value); onTrack?.(Number(value))
      onSelect(`/api/transcode/${encodeURIComponent(data.id)}/manifest.m3u8`)
    } catch (error) { if (current === generation.current) setError(error instanceof Error ? error.message : 'Could not prepare audio.') }
    finally { if (current === generation.current) setBusy(false) }
  }
  return <div className="video-audio"><label>Audio track <select aria-label="Audio track" value={selected} disabled={busy || !tracks.length} onChange={event => { void select(event.target.value) }}><option value="">Original / default audio</option>{tracks.map(track => <option key={track.index} value={track.index}>{track.label || `Track ${track.index}`} · {track.codec}{track.default ? ' · Default' : ''} → AAC</option>)}</select></label>{busy && <p role="status">Starting segmented H.264 / AAC playback… Originals are preserved.</p>}{error && <p role="status">{error}</p>}{selected && <button className="track-play" disabled={busy} onClick={() => { void select(selected) }}>Restart audio stream</button>}</div>
}
