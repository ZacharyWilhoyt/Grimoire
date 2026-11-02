import { useEffect, useRef, useState } from 'react'
import { RefreshCw } from 'lucide-react'
import { apiFetch } from './api'
type Job = { id: string; status: string; total: number; processed: number; skipped: number; warnings: number; message: string | null; stale: boolean }
export default function SourceMetadataTools({ sourceId, onUpdated }: { sourceId: string; onUpdated: () => void }) {
  const [job, setJob] = useState<Job | null>(null)
  const [busy, setBusy] = useState(false)
  const [force, setForce] = useState(false)
  const [error, setError] = useState('')
  const [statusError, setStatusError] = useState('')
  const [statusKnown, setStatusKnown] = useState(false)
  const [refresh, setRefresh] = useState(0)
  const callback = useRef(onUpdated); callback.current = onUpdated
  const notified = useRef('')
  useEffect(() => {
    let active = true
    const controller = new AbortController()
    setStatusKnown(false); setStatusError(''); setJob(null)
    let timer: number | undefined
    const load = async () => {
      try {
        const response = await apiFetch(`/api/sources/${encodeURIComponent(sourceId)}/metadata/job`, { signal: controller.signal })
        const data = await response.json()
        if (!response.ok) throw new Error(data.error ?? 'Could not load the metadata scan status.')
        if (!active) return
        setJob(data as Job | null); setStatusKnown(true); setStatusError('')
        if (data?.status === 'running' && !data.stale) timer = window.setTimeout(() => { void load() }, 2500)
        if (data && data.status !== 'running' && notified.current !== data.id) { notified.current = data.id; callback.current() }
      } catch (error) { if (active) { setStatusKnown(false); setStatusError(error instanceof Error ? error.message : 'Could not load scan status.') } }
    }
    void load()
    return () => { active = false; controller.abort(); window.clearTimeout(timer) }
  }, [sourceId, refresh])
  const start = async () => {
    setBusy(true); setError('')
    try {
      const response = await apiFetch(`/api/sources/${encodeURIComponent(sourceId)}/metadata/scan`, { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ force }) })
      const data = await response.json()
      if (!response.ok) throw new Error(data.error ?? 'Could not start metadata gathering.')
      setStatusKnown(false); setRefresh(value => value + 1)
    } catch (error) { setError(error instanceof Error ? error.message : 'Could not start metadata gathering.') }
    finally { setBusy(false) }
  }
  const stop = async () => {
    if (!job) return
    setBusy(true); setError('')
    try {
      const response = await apiFetch(`/api/sources/${encodeURIComponent(sourceId)}/metadata/job/${encodeURIComponent(job.id)}/cancel`, { method: 'POST' })
      if (!response.ok) { const data = await response.json(); throw new Error(data.error ?? 'Could not stop metadata gathering.') }
      setStatusKnown(false); setRefresh(value => value + 1)
    } catch (error) { setError(error instanceof Error ? error.message : 'Could not stop metadata gathering.') }
    finally { setBusy(false) }
  }
  const running = job?.status === 'running' && !job.stale
  return <div className="source-metadata-tools">
    <div>
      <button className="track-play" disabled={busy || !statusKnown || running} onClick={() => { void start() }}><RefreshCw size={13}/> Gather local metadata</button>
      {job?.status === 'running' && <button className="track-play" disabled={busy || !statusKnown} onClick={() => { void stop() }}>Stop scan</button>}
      <label><input type="checkbox" checked={force} disabled={busy || !statusKnown || running} onChange={event => setForce(event.target.checked)}/> Re-read unchanged files</label>
    </div>
    {!statusKnown && !statusError && <p role="status">Checking scan status…</p>}
    {job && <p role="status">{!statusKnown && 'Last known status: '}{job.stale ? 'Previous scan stopped responding. You can start it again.' : `${job.status}: ${job.processed}/${job.total} files · ${job.skipped} unchanged · ${job.warnings} warnings`}{job.message ? ` · ${job.message}` : ''}</p>}
    {statusError && <p className="video-error" role="alert">{statusError} <button className="track-play" disabled={busy} onClick={() => setRefresh(value => value + 1)}>Retry scan status</button></p>}
    {error && <p className="video-error" role="alert">{error}</p>}
    <small>Reads embedded metadata, cover art, and filename hints. Online matching is reviewed separately per item.</small>
  </div>
}
