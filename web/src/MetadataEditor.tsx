import MetadataCredits from './MetadataCredits'
import { useEffect, useRef, useState, type FormEvent } from 'react'
import { X, Search, RefreshCw } from 'lucide-react'
import { apiFetch } from './api'
import { displayTitle, type MediaItem, type MetadataFields, type MetadataView } from './media'

type Provider = 'tmdb_movie' | 'tmdb_tv' | 'musicbrainz' | 'openlibrary'
type Status = { provider: Provider; enabled: boolean; message: string | null }
type Candidate = { id: string; title: string; creator: string | null; collection: string | null; year: number | null }
type Review = { review_token: string; provider: Provider; provider_id: string; fields: MetadataFields; has_artwork: boolean }
const providerName = (provider: Provider) => ({ tmdb_movie: 'TMDB movies', tmdb_tv: 'TMDB television', musicbrainz: 'MusicBrainz recordings', openlibrary: 'Open Library books' })[provider]
const providersFor = (kind: string): Provider[] => kind === 'video' ? ['tmdb_movie', 'tmdb_tv'] : kind === 'music' ? ['musicbrainz'] : ['openlibrary']
async function call<T>(url: string, init?: RequestInit): Promise<T> {
  const response = await apiFetch(url, init)
  const data = await response.json()
  if (!response.ok) throw new Error(data.error ?? 'Metadata operation failed.')
  return data as T
}
const json = (value: unknown) => ({ method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(value) })
export default function MetadataEditor({ item, onClose, onUpdated }: { item: MediaItem; onClose: () => void; onUpdated: () => void }) {
  const dialog = useRef<HTMLDialogElement>(null)
  const [view, setView] = useState<MetadataView | null>(null)
  const [fields, setFields] = useState<MetadataFields>({})
  const [genres, setGenres] = useState('')
  const [artwork, setArtwork] = useState('auto')
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState('')
  const [message, setMessage] = useState('')
  const [provider, setProvider] = useState<Provider>(providersFor(item.kind)[0])
  const [statuses, setStatuses] = useState<Status[]>([])
  const [query, setQuery] = useState(displayTitle(item))
  const [results, setResults] = useState<Candidate[]>([])
  const [searched, setSearched] = useState(false)
  const [review, setReview] = useState<Review | null>(null)
  const [replaceCustom, setReplaceCustom] = useState(false)
  const endpoint = `/api/sources/${encodeURIComponent(item.source_id)}/metadata`
  const pathQuery = new URLSearchParams({ path: item.relative_path })
  const accept = (view: MetadataView) => { setView(view); setFields(view.fields); setGenres((view.fields.genres ?? []).join('\n')); setArtwork(view.artwork_preference) }
  useEffect(() => {
    const element = dialog.current; element?.showModal()
    let active = true
    call<MetadataView>(`${endpoint}?${new URLSearchParams({ path: item.relative_path })}`).then(view => { if (active) accept(view) }).catch(error => { if (active) setError(error instanceof Error ? error.message : 'Could not load metadata.') })
    call<Status[]>('/api/metadata/providers').then(statuses => { if (active) setStatuses(statuses) }).catch(() => {})
    return () => { active = false; element?.close() }
  }, [endpoint, item.relative_path])
  const action = async (work: () => Promise<void>) => { setBusy(true); setError(''); setMessage(''); try { await work() } catch (error) { setError(error instanceof Error ? error.message : 'Metadata operation failed.') } finally { setBusy(false) } }
  const save = (event: FormEvent) => { event.preventDefault(); void action(async () => {
    const result = await call<MetadataView>(endpoint, { ...json({ path: item.relative_path, fields: { ...fields, genres: genres.split('\n').map(value => value.trim()).filter(Boolean) }, artwork_preference: artwork }), method: 'PUT' })
    accept(result); onUpdated(); setMessage('Custom metadata saved. The original file and its filename are unchanged.')
  }) }
  const extract = () => { void action(async () => {
    const result = await call<{ metadata: MetadataView; warnings: string[] }>(`${endpoint}/extract`, json({ path: item.relative_path }))
    accept(result.metadata); onUpdated(); setMessage(['Local metadata gathered.', ...result.warnings, result.metadata.has_custom ? 'Your custom metadata is still active.' : ''].filter(Boolean).join(' '))
  }) }
  const reset = () => { void action(async () => {
    const result = await call<MetadataView>(`${endpoint}?${pathQuery}`, { method: 'DELETE' })
    accept(result); onUpdated(); setMessage('Using gathered metadata. Custom field overrides and artwork preference were reset.')
  }) }
  const search = (event: FormEvent) => { event.preventDefault(); void action(async () => {
    setReview(null); setResults([]); setSearched(false)
    const result = await call<Candidate[]>(`${endpoint}/search`, json({ path: item.relative_path, provider, query }))
    setResults(result); setSearched(true)
  }) }
  const preview = (candidate: Candidate) => { void action(async () => {
    setReview(null)
    const result = await call<Review>(`${endpoint}/preview`, json({ path: item.relative_path, provider, id: candidate.id }))
    setReview(result); setReplaceCustom(false)
  }) }
  const apply = () => { if (!review) return; void action(async () => {
    const result = await call<{ metadata: MetadataView; warnings: string[] }>(`${endpoint}/apply`, json({ path: item.relative_path, review_token: review.review_token, replace_custom: replaceCustom }))
    accept(result.metadata); setReview(null); onUpdated(); setMessage(['Reviewed match applied.', ...result.warnings, result.metadata.has_custom ? 'Your custom metadata remains active. Use gathered metadata to display the match.' : ''].filter(Boolean).join(' '))
  }) }
  const field = (key: 'title' | 'creator' | 'collection', label: string) => <label>{label}<input maxLength={300} value={fields[key] ?? ''} disabled={busy || !view?.can_edit} onChange={event => setFields({ ...fields, [key]: event.target.value })}/></label>
  const number = (key: 'year' | 'season' | 'episode' | 'track' | 'disc' | 'volume', label: string) => <label>{label}<input type="number" min={key === 'year' ? 1 : 0} max={key === 'year' ? 9999 : 100000} step={key === 'volume' ? 'any' : 1} value={fields[key] ?? ''} disabled={busy || !view?.can_edit} onChange={event => setFields({ ...fields, [key]: event.target.value === '' ? null : Number(event.target.value) })}/></label>
  const selected = statuses.find(status => status.provider === provider)
  return <dialog ref={dialog} className="video-backdrop" aria-labelledby="metadata-title" onCancel={event => { event.preventDefault(); onClose() }}><section className="metadata-panel">
    <header className="video-header"><div><h2 id="metadata-title">{view?.can_edit ? 'Edit metadata' : 'Media details'}</h2><span>{displayTitle(item)}</span></div><button aria-label="Close metadata" onClick={onClose}><X size={21}/></button></header>
    <div className="metadata-content"><p className="metadata-original">Original file: {item.relative_path}</p>
      {!view && !error && <p role="status">Loading metadata…</p>}
      {view && <>
        <div className="metadata-provenance"><span>Active metadata: {view.origin}</span>{view.artwork_url && <img src={view.artwork_url} alt="Current artwork"/>}</div>
        <form className="metadata-form" onSubmit={save}>
          {field('title', 'Display title')}{field('creator', item.kind === 'music' ? 'Artist / album artist' : item.kind === 'book' ? 'Author / writer' : 'Creator')}{field('collection', item.kind === 'music' ? 'Album' : 'Series / collection')}
          <div className="metadata-numbers">{number('year', 'Year')}{item.kind === 'video' && <>{number('season', 'Season')}{number('episode', 'Episode')}</>}{item.kind === 'music' && <>{number('disc', 'Disc')}{number('track', 'Track')}</>}{item.kind === 'book' && number('volume', 'Volume / series position')}</div>
          <label>Genres / subjects<textarea rows={3} maxLength={1400} value={genres} disabled={busy || !view.can_edit} onChange={event => setGenres(event.target.value)}/><small>One tag per line. Tags are normalized for browsing and filtering.</small></label>
          <label>Description<textarea rows={4} maxLength={4000} value={fields.description ?? ''} disabled={busy || !view.can_edit} onChange={event => setFields({ ...fields, description: event.target.value })}/></label>
          {view.can_edit && <><label>Artwork<select value={artwork} disabled={busy} onChange={event => setArtwork(event.target.value)}><option value="auto">Matched artwork, with local fallback</option><option value="local">Local artwork only</option><option value="matched">Matched artwork only</option><option value="none">No artwork</option></select></label><button className="connect-submit" type="submit" disabled={busy}>Save custom metadata</button></>}
        </form>
        {view.can_edit && <>
          <div className="metadata-local"><button disabled={busy} className="track-play" onClick={extract}><RefreshCw size={13}/> Read local tags and artwork</button>{view.has_custom && <button disabled={busy} className="track-play" onClick={reset}>Use gathered metadata</button>}</div>
          <p className="metadata-help">Custom metadata is a complete owner-edited version. Local rescans and online matches preserve it until you choose to replace it.</p>
          <section className="metadata-online"><h3>Reviewed online matching</h3><p>Searching sends the query below to the selected provider. Preview a candidate and confirm before applying it. Files are never uploaded.</p>
            <form className="metadata-form" onSubmit={search}><label>Provider<select value={provider} disabled={busy} onChange={event => { setProvider(event.target.value as Provider); setResults([]); setReview(null); setSearched(false) }}>{providersFor(item.kind).map(provider => <option key={provider} value={provider}>{providerName(provider)}</option>)}</select></label><label>Search query<input value={query} maxLength={200} required disabled={busy} onChange={event => setQuery(event.target.value)}/></label>{selected?.message && <p className="metadata-help">{selected.message}</p>}<button className="track-play" disabled={busy || !selected?.enabled}><Search size={14}/> Search provider</button></form>
            {searched && !results.length && <p>No matching results. Try a different title.</p>}
            <div className="metadata-matches">{results.map(result => <article key={result.id}><div><strong>{result.title}</strong><span>{[result.creator, result.collection, result.year].filter(Boolean).join(' · ')}</span></div><button className="track-play" disabled={busy} onClick={() => preview(result)}>Preview match</button></article>)}</div>
            {review && <div className="metadata-review"><h4>Review before applying</h4><strong>{review.fields.title}</strong><p>{[review.fields.creator, review.fields.collection, review.fields.year].filter(Boolean).join(' · ')}</p><p>{(review.fields.genres ?? []).join(', ')}</p><p>{review.fields.description}</p><small>{providerName(review.provider)} · {review.provider_id} · {review.has_artwork ? 'Provider artwork will be requested on apply.' : 'No provider artwork specified.'}</small>{view.has_custom && <label className="grant-toggle"><input type="checkbox" checked={replaceCustom} onChange={event => setReplaceCustom(event.target.checked)}/><span>Replace my custom metadata with this match</span></label>}<button className="connect-submit" disabled={busy} onClick={apply}>Apply reviewed match</button></div>}

          </section>
        </>}
      </>}
      <MetadataCredits/>
      {busy && <p className="metadata-help" role="status">Working… provider lookups and local extraction can take a moment.</p>}{error && <p className="video-error" role="alert">{error}</p>}{message && <p className="permission-success" role="status">{message}</p>}
    </div>
  </section></dialog>
}
