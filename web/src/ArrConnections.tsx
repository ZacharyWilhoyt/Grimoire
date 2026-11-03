import { useEffect, useState, type FormEvent } from 'react'
import { apiFetch } from './api'
export type ArrConnection = { id: string; name: string; kind: 'sonarr' | 'radarr' | 'lidarr'; base_url: string; remote_root: string; source_subpath: string; enabled: boolean; has_api_key: boolean; usable: boolean }
type Settings = { configured: boolean; online: boolean; connections: ArrConnection[] }
// randomUUID requires a secure context; getRandomValues also works on LAN HTTP.
function connectionId() {
  if (typeof crypto.randomUUID === 'function') return crypto.randomUUID()
  const bytes = crypto.getRandomValues(new Uint8Array(16))
  bytes[6] = (bytes[6] & 0x0f) | 0x40
  bytes[8] = (bytes[8] & 0x3f) | 0x80
  const hex = Array.from(bytes, byte => byte.toString(16).padStart(2, '0')).join('')
  return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20)}`
}
const blank = () => ({ id: connectionId(), name: '', kind: 'sonarr' as ArrConnection['kind'], base_url: '', remote_root: '', source_subpath: '', enabled: true, api_key: '' })
async function call<T>(url: string, options?: RequestInit): Promise<T> {
  const response = await apiFetch(url, options)
  if (response.status === 204) return undefined as T
  const data = await response.json()
  if (!response.ok) throw new Error(data.error ?? 'Could not update Arr connections.')
  return data as T
}
export default function ArrConnections({ sourceId }: { sourceId: string }) {
  const [open, setOpen] = useState(false)
  const [settings, setSettings] = useState<Settings | null>(null)
  const [form, setForm] = useState(blank)
  const [showForm, setShowForm] = useState(false)
  const [editing, setEditing] = useState(false)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState('')
  const [message, setMessage] = useState('')
  const endpoint = `/api/sources/${encodeURIComponent(sourceId)}/arr`
  useEffect(() => {
    if (!open) return
    let active = true
    call<Settings>(endpoint).then(data => { if (active) { setSettings(data); if (!data.connections.length) setShowForm(true) } }).catch(error => { if (active) setError(error instanceof Error ? error.message : 'Could not load connections.') })
    return () => { active = false }
  }, [open, endpoint])
  const action = async (work: () => Promise<void>) => { setBusy(true); setError(''); setMessage(''); try { await work() } catch (error) { setError(error instanceof Error ? error.message : 'Arr operation failed.') } finally { setBusy(false) } }
  const save = (event: FormEvent) => { event.preventDefault(); void action(async () => {
    const { id, api_key, ...fields } = form
    await call(`${endpoint}/${id}`, { method: 'PUT', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ ...fields, api_key: api_key || null }) })
    setForm(blank()); setEditing(false); setShowForm(false); setSettings(await call<Settings>(endpoint)); setMessage('Connection saved. Test it, then use an item’s metadata panel to preview an Arr match.')
  }) }
  return <section className="arr-settings"><button className="track-play" aria-expanded={open} onClick={() => setOpen(!open)}>Optional Arr connections {open ? '−' : '+'}</button>{open && <div className="arr-content">
    <p>Local metadata and reviewed online matching remain the default. Connect Sonarr, Radarr, or Lidarr to reuse metadata for files already in their libraries. These connections only read metadata.</p>
    {settings && !settings.configured && <p className="catalog-notice">The server operator must configure GRIMOIRE_INTEGRATION_KEY and GRIMOIRE_ARR_ALLOWED_URLS before connections can be saved. See the Arr integration guide in the project documentation.</p>}
    {settings && !settings.online && <p className="catalog-notice">Online metadata is disabled by the server. Saved Arr connections cannot be tested or used until it is enabled.</p>}
    <div className="arr-connection-list">{settings?.connections.map(connection => <article key={connection.id}><div><strong>{connection.name}</strong><span>{connection.kind} · {connection.enabled ? connection.usable ? 'Enabled' : 'Needs server configuration' : 'Disabled'}</span><small>{connection.base_url}</small><small>{connection.remote_root} → {connection.source_subpath || 'Source root'}</small></div><div className="arr-buttons"><button className="track-play" disabled={busy || !connection.usable || !settings.online} onClick={() => { void action(async () => { const result = await call<{ app: string; version: string; api: string }>(`${endpoint}/${connection.id}/test`, { method: 'POST' }); setMessage(`Connected to ${result.app} ${result.version ?? ''} (${result.api}). Folder matching is checked when previewing a file.`) }) }}>Test connection</button><button className="track-play" disabled={busy} onClick={() => { setForm({ id: connection.id, name: connection.name, kind: connection.kind, base_url: connection.base_url, remote_root: connection.remote_root, source_subpath: connection.source_subpath, enabled: connection.enabled, api_key: '' }); setEditing(true); setShowForm(true); setMessage('') }}>Edit</button><button className="disconnect-button" disabled={busy} onClick={() => { void action(async () => { await call(`${endpoint}/${connection.id}`, { method: 'DELETE' }); setSettings(await call<Settings>(endpoint)); if (form.id === connection.id) { setForm(blank()); setEditing(false); setShowForm(false) } setMessage('Connection removed. Previously applied metadata is retained.') }) }}>Remove connection</button></div></article>)}</div>
    {!showForm && <button className="track-play arr-add" disabled={busy || !settings?.configured} onClick={() => { setForm(blank()); setEditing(false); setShowForm(true) }}>Add a connection</button>}
    {showForm && <><h3>{editing ? 'Edit connection' : 'Add a connection'}</h3>
    <form className="metadata-form" onSubmit={save}><label>Connection name<input required maxLength={80} value={form.name} disabled={busy} onChange={event => setForm({ ...form, name: event.target.value })}/></label><label>Arr app<select value={form.kind} disabled={busy} onChange={event => setForm({ ...form, kind: event.target.value as ArrConnection['kind'] })}><option value="sonarr">Sonarr — series / anime</option><option value="radarr">Radarr — movies</option><option value="lidarr">Lidarr — music</option></select></label><label>Base URL<input type="url" required placeholder="https://sonarr.example.com/sonarr/" maxLength={2048} value={form.base_url} disabled={busy} onChange={event => setForm({ ...form, base_url: event.target.value })}/><small>Must match an operator-approved base URL, including any reverse-proxy prefix. Grimoire’s server must be able to reach it.</small></label><label>API key<input type="password" autoComplete="new-password" required={!editing} maxLength={256} value={form.api_key} disabled={busy} onChange={event => setForm({ ...form, api_key: event.target.value })}/><small>{editing ? 'Leave blank to retain the stored key. Re-enter it when changing the app or URL.' : 'Stored encrypted on the server and never returned to the browser.'}</small></label><label>Media folder as seen by Arr<input required placeholder="/tv" maxLength={4096} value={form.remote_root} disabled={busy} onChange={event => setForm({ ...form, remote_root: event.target.value })}/></label><label>Folder inside this Grimoire source<input placeholder="Leave blank for the source root" maxLength={4096} value={form.source_subpath} disabled={busy} onChange={event => setForm({ ...form, source_subpath: event.target.value })}/><small>For example: Arr /tv maps to this source’s root, or to a relative folder such as shows. Files are matched by their full mapped path, never just their title.</small></label><label className="grant-toggle"><input type="checkbox" checked={form.enabled} disabled={busy} onChange={event => setForm({ ...form, enabled: event.target.checked })}/> Enable this connection</label><div className="arr-buttons"><button className="track-play" type="submit" disabled={busy || !settings?.configured}>Save connection</button><button className="track-play" type="button" disabled={busy} onClick={() => { setEditing(false); setShowForm(false); setForm(blank()) }}>Cancel</button></div></form></>}
    {busy && <p role="status">Working…</p>}{error && <p className="video-error" role="alert">{error}</p>}{message && <p className="permission-success" role="status">{message}</p>}
  </div>}</section>
}
