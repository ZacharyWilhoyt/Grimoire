import { useEffect, useState } from 'react'
import { apiFetch } from './api'
import type { ArrConnection } from './ArrConnections'
import type { MediaItem, MetadataFields } from './media'

type Review = {
  review_token: string
  title: string
  synopsis: string | null
  has_artwork: boolean
  episodes: { path: string; fields: MetadataFields }[]
}
type Result = { episodes_updated: number; synopsis_updated: boolean; warnings: string[] }

async function request<T>(url: string, body?: object): Promise<T> {
  const response = await apiFetch(url, body ? {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(body),
  } : undefined)
  const data = await response.json()
  if (!response.ok) throw new Error(data.error ?? 'Could not review the Sonarr series.')
  return data as T
}

export default function SeriesArrReview({ item, endpoint, onApplied }: {
  item: MediaItem
  endpoint: string
  onApplied: () => Promise<void>
}) {
  const [connections, setConnections] = useState<ArrConnection[] | null>(null)
  const [connection, setConnection] = useState('')
  const [review, setReview] = useState<Review | null>(null)
  const [synopsis, setSynopsis] = useState(true)
  const [episodes, setEpisodes] = useState(true)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState('')
  const [message, setMessage] = useState('')

  useEffect(() => {
    let active = true
    void request<{ online: boolean; connections: ArrConnection[] }>(`/api/sources/${encodeURIComponent(item.source_id)}/arr`)
      .then(data => {
        if (!active) return
        const options = data.online ? data.connections.filter(value => value.kind === 'sonarr' && value.usable) : []
        setConnections(options)
        setConnection(options[0]?.id ?? '')
      })
      .catch(error => { if (active) setError(error instanceof Error ? error.message : 'Could not load Sonarr connections.') })
    return () => { active = false }
  }, [item.source_id])

  async function run(work: () => Promise<void>) {
    setBusy(true)
    setError('')
    setMessage('')
    try { await work() }
    catch (error) { setError(error instanceof Error ? error.message : 'Series review failed.') }
    finally { setBusy(false) }
  }

  return <section className="metadata-form" aria-label="Series Arr metadata">
    <h3>From your Sonarr library</h3>
    {connections?.length === 0 && <p className="metadata-help">Enable a Sonarr connection in the source settings to preview this series.</p>}
    {!!connections?.length && <>
      <label>Sonarr connection<select value={connection} disabled={busy}
        onChange={event => { setConnection(event.target.value); setReview(null) }}>
        {connections.map(option => <option value={option.id} key={option.id}>{option.name}</option>)}
      </select></label>
      <button className="track-play" disabled={busy || !connection} onClick={() => void run(async () => {
        setReview(null)
        const result = await request<Review>(`${endpoint}/arr/${encodeURIComponent(connection)}/preview`, { path: item.relative_path })
        setReview(result)
        setSynopsis(!!result.synopsis)
        setEpisodes(result.episodes.length > 0)
      })}>Preview series from Arr library</button>
    </>}
    {review && <div className="metadata-form">
      <strong>{review.title}</strong>
      {review.synopsis && <p className="metadata-help series-arr-synopsis">{review.synopsis}</p>}
      <label><span className="series-arr-option"><input type="checkbox" checked={synopsis} disabled={busy || !review.synopsis}
        onChange={event => setSynopsis(event.target.checked)}/> Save synopsis for this series</span></label>
      <label><span className="series-arr-option"><input type="checkbox" checked={episodes} disabled={busy || !review.episodes.length}
        onChange={event => setEpisodes(event.target.checked)}/> Apply metadata to {review.episodes.length} matched {review.episodes.length === 1 ? 'file' : 'files'}</span></label>
      {review.episodes.length > 0 && <details>
        <summary>Review matched episodes</summary>
        <ul className="series-arr-episodes">{review.episodes.map(episode => <li key={episode.path}>
          <strong>{episode.fields.title || episode.path}</strong> <small>{episode.path}</small>
        </li>)}</ul>
      </details>}
      <small>Episode imports preserve manual edits and use Sonarr’s artwork when available.</small>
      <button className="track-play" disabled={busy || (!synopsis && !episodes)} onClick={() => void run(async () => {
        const result = await request<Result>(`${endpoint}/arr/apply`, {
          path: item.relative_path, review_token: review.review_token, synopsis, episodes,
        })
        setReview(null)
        setMessage([
          result.synopsis_updated ? 'Series synopsis saved.' : '',
          result.episodes_updated ? `Updated ${result.episodes_updated} matched ${result.episodes_updated === 1 ? 'file' : 'files'}.` : '',
          ...result.warnings,
        ].filter(Boolean).join(' '))
        await onApplied()
      })}>Apply reviewed series metadata</button>
    </div>}
    {busy && <p role="status">Working with Sonarr…</p>}
    {error && <p role="alert" className="video-error">{error}</p>}
    {message && <p role="status" className="permission-success">{message}</p>}
  </section>
}
