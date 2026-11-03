import { useEffect, useRef, useState } from 'react'
import { apiFetch } from './api'
type Track = { index: number; label: string; codec: string; default: boolean }
type Props = { sourceId: string; path: string; video: HTMLVideoElement | null; onSelect: (url: string | null) => void; profile?: {height:number}; onTrack?: (index:number|null)=>void }
export default function VideoAudio({ sourceId, path, video, onSelect, profile = {height:0}, onTrack }: Props) {
  const [tracks, setTracks] = useState<Track[]>([]), [selected, setSelected] = useState(''), [busy, setBusy] = useState(false), [error, setError] = useState('')
  const generation = useRef(0)
  useEffect(() => {
    const controller = new AbortController()
    void apiFetch(`/api/sources/${encodeURIComponent(sourceId)}/audio-tracks?${new URLSearchParams({ path })}`, { signal: controller.signal }).then(async response => {
      if (!response.ok) throw new Error('Audio tracks could not be loaded. Try again shortly.')
      const data = await response.json() as Track[]
      if (!controller.signal.aborted) setTracks(data)
    }).catch(error => { if (!controller.signal.aborted) setError(error instanceof Error ? error.message : 'Audio tracks could not be loaded.') })
    return () => { controller.abort(); generation.current++ }
  }, [sourceId, path])
  async function select(value: string) {
    const current = ++generation.current
    setError('')
    video?.pause(); setBusy(true)
    try {
      const response = await apiFetch('/api/transcode', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ source_id: sourceId, path, ...profile, audio: value===''?null:Number(value) }) })
      const data = await response.json() as { id?: string; error?: string }
      if (!response.ok || !data.id) throw new Error(data.error ?? 'Could not prepare this audio track.')
      if (current !== generation.current) { void apiFetch(`/api/transcode/${encodeURIComponent(data.id)}`,{method:'DELETE'}).catch(()=>{}); return }
      setSelected(value); onTrack?.(value===''?null:Number(value))
      onSelect(`/api/transcode/${encodeURIComponent(data.id)}/manifest.m3u8`)
    } catch (error) { if (current === generation.current) setError(error instanceof Error ? error.message : 'Could not prepare audio.') }
    finally { if (current === generation.current) setBusy(false) }
  }
  return <div className="video-audio"><p>Playback audio: AAC · up to 192 kbps</p><div className="track-options" role="group" aria-label="Audio tracks"><button aria-pressed={selected===''} disabled={busy} onClick={()=>{void select('')}}>Source default audio</button>{tracks.map(track=><button key={track.index} aria-pressed={selected===String(track.index)} disabled={busy} onClick={()=>{void select(String(track.index))}}>{track.label || `Track ${track.index}`} · {track.codec}{track.default?' · Default':''}</button>)}</div>{busy&&<p role="status">Starting playback…</p>}{error&&<p role="status">{error}</p>}</div>
}
