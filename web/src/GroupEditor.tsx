import { useModalDialog } from './useModalDialog'
import { useEffect, useRef, useState } from 'react'
import { X, Upload } from 'lucide-react'
import { apiFetch } from './api'
import type { GroupView, MediaItem } from './media'
type Settings = { group: GroupView; genres: string[] | null; artwork_url: string | null; effective_genres: string[]; effective_artwork_url: string | null }
export default function GroupEditor({ item, onClose, onUpdated }: { item: MediaItem; onClose: () => void; onUpdated: () => void }) {
  const dialog = useModalDialog(), picker = useRef<HTMLInputElement>(null)
  const [settings, setSettings] = useState<Settings | null>(null)
  const [genres, setGenres] = useState('')
  const [busy, setBusy] = useState(false), [error, setError] = useState(''), [message, setMessage] = useState('')
  const group = item.metadata!.group!
  const endpoint = `/api/sources/${encodeURIComponent(item.source_id)}/groups/${group.id}`
  const query = new URLSearchParams({ path: item.relative_path })
  const accept = (data: Settings) => { setSettings(data); setGenres(data.effective_genres.join('\n')) }
  async function call(url: string, options?: RequestInit): Promise<Settings> {
    const response = await apiFetch(url, options)
    if (response.status === 413) throw new Error('The image must be at most 8 MiB.')
    const data = await response.json()
    if (!response.ok) throw new Error(data.error ?? 'Could not update the album/series.')
    return data as Settings
  }
  useEffect(() => {
    let active = true
    call(`${endpoint}?${new URLSearchParams({ path: item.relative_path })}`).then(data => { if (active) accept(data) }).catch(error => { if (active) setError(error instanceof Error ? error.message : 'Could not load the group.') })
    return () => { active = false }
  }, [endpoint, item.relative_path])
  const action = async (work: () => Promise<Settings>, success: string) => {
    setBusy(true); setError(''); setMessage('')
    try { accept(await work()); setMessage(success); onUpdated() } catch (error) { setError(error instanceof Error ? error.message : 'Group operation failed.') } finally { setBusy(false) }
  }
  const upload = (file: File) => {
    if (file.size > 8 * 1024 * 1024) { setError('The image must be at most 8 MiB.'); return }
    void action(() => call(`${endpoint}/artwork?${query}`, { method: 'POST', headers: { 'Content-Type': file.type || 'application/octet-stream' }, body: file }), 'Custom artwork now applies to every member of this album/series.')
  }
  return <dialog className="video-backdrop" ref={dialog} aria-labelledby="group-editor-title" onCancel={event => { event.preventDefault(); onClose() }}><section className="metadata-panel"><header className="video-header"><div><h2 id="group-editor-title">Album / series settings</h2><span>{group.label}</span></div><button onClick={onClose} aria-label="Close group settings"><X size={21}/></button></header><div className="metadata-content">
    <p className="metadata-help">Changes apply to all current and future members of this group in {item.source_name}, including members outside this view. Individual titles and original files stay unchanged.</p>
    {!settings && !error && <p role="status">Loading group settings…</p>}
    {settings?.group.can_edit && <>
      <div className="group-cover-preview">{settings.effective_artwork_url && <img src={settings.effective_artwork_url!} alt="Album or series cover"/>}<div><input ref={picker} className="cover-file-input" type="file" aria-label="Custom group artwork" accept="image/jpeg,image/png,image/webp,image/gif" disabled={busy} onChange={event => { const file = event.target.files?.[0]; if (file) upload(file); event.target.value = '' }}/><button className="track-play" disabled={busy} onClick={() => picker.current?.click()}><Upload size={14}/> Upload poster / album cover</button><p className="metadata-help">JPEG, PNG, WebP or GIF. Up to 8 MiB and 4096 pixels per side. Images are resized and stored privately.</p>{settings.group.has_custom_artwork && <button className="track-play" disabled={busy} onClick={() => { void action(() => call(`${endpoint}/artwork?${query}`, { method: 'DELETE' }), 'Custom group artwork removed. Individual artwork is active again.') }}>Use individual artwork</button>}</div></div>
      <form className="metadata-form" onSubmit={event => { event.preventDefault(); void action(() => call(`${endpoint}/genres`, { method: 'PUT', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ path: item.relative_path, genres: genres.split('\n').map(tag => tag.trim()).filter(Boolean) }) }), 'Group tags now apply to every member.') }}><label>Group genres / tags<textarea rows={5} value={genres} maxLength={1400} disabled={busy} onChange={event => setGenres(event.target.value)}/><small>One tag per line, up to 20. This replaces the displayed tags for every member; an empty list clears them. Without a group override, the initial list comes from one member and other members may differ.</small></label><button className="track-play" disabled={busy} type="submit">Save tags for entire group</button>{settings.group.has_custom_genres && <button className="track-play" disabled={busy} type="button" onClick={() => { void action(() => call(`${endpoint}/genres?${query}`, { method: 'DELETE' }), 'Group tag override removed. Individual tags are active again.') }}>Use individual tags</button>}</form>
    </>}
    {busy && <p role="status" className="metadata-help">Saving group settings…</p>}{error && <p className="video-error" role="alert">{error}</p>}{message && <p className="permission-success" role="status">{message}</p>}
  </div></section></dialog>
}
