import { useEffect, useRef, useState, type FormEvent } from 'react'
import { X } from 'lucide-react'
import { apiFetch } from './api'

export type AcquisitionPolicy = 'auto_deny' | 'manual_approval' | 'auto_accept'
export type Grant = { can_discover: boolean; can_access_media: boolean; acquisition_policy: AcquisitionPolicy }
type Permission = Grant & { user_id: string; username: string }
const denied = (): Grant => ({ can_discover: false, can_access_media: false, acquisition_policy: 'auto_deny' })
export const policyLabel = (policy: AcquisitionPolicy) => ({ auto_deny: 'Auto-deny', manual_approval: 'Manual approval', auto_accept: 'Auto-accept' })[policy]

export default function SourcePermissions({ source, ownerUsername, onClose, onSaved }: { source: { id: string; name: string }; ownerUsername: string; onClose: () => void; onSaved: () => void }) {
  const dialog = useRef<HTMLDialogElement>(null)
  const [permissions, setPermissions] = useState<Permission[]>([])
  const [loading, setLoading] = useState(true)
  const [busy, setBusy] = useState(false)
  const [username, setUsername] = useState('')
  const [grant, setGrant] = useState<Grant>(denied)
  const [error, setError] = useState('')
  const [message, setMessage] = useState('')
  const owner = username.trim().toLowerCase() === ownerUsername
  const effective = owner ? { ...grant, can_discover: true, can_access_media: true } : grant
  const endpoint = `/api/sources/${encodeURIComponent(source.id)}/permissions`
  useEffect(() => {
    const element = dialog.current
    element?.showModal()
    let active = true
    apiFetch(endpoint).then(async response => {
      const data = await response.json()
      if (!response.ok) throw new Error(data.error ?? 'Could not load permissions.')
      if (active) setPermissions(data as Permission[])
    }).catch(error => { if (active) setError(error instanceof Error ? error.message : 'Could not load permissions.') }).finally(() => { if (active) setLoading(false) })
    return () => { active = false; element?.close() }
  }, [endpoint])
  const reload = async () => {
    const response = await apiFetch(endpoint)
    const data = await response.json()
    if (!response.ok) throw new Error(data.error ?? 'Could not refresh permissions.')
    setPermissions(data as Permission[])
    onSaved()
  }
  const save = async (event: FormEvent) => {
    event.preventDefault(); setBusy(true); setError(''); setMessage('')
    try {
      const response = await apiFetch(`${endpoint}/${encodeURIComponent(username.trim())}`, { method: 'PUT', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(effective) })
      if (!response.ok) { const data = await response.json(); throw new Error(data.error ?? 'Could not save permissions.') }
      await reload()
      setMessage(`Permissions saved for ${username.trim().toLowerCase()}.`)
      setUsername(''); setGrant(denied())
    } catch (error) { setError(error instanceof Error ? error.message : 'Could not save permissions.') }
    finally { setBusy(false) }
  }
  const revoke = async (name: string) => {
    setBusy(true); setError(''); setMessage('')
    try {
      const response = await apiFetch(`${endpoint}/${encodeURIComponent(name)}`, { method: 'DELETE' })
      if (!response.ok) { const data = await response.json(); throw new Error(data.error ?? 'Could not remove permissions.') }
      await reload()
      if (name === username.trim().toLowerCase()) { setUsername(''); setGrant(denied()) }
      setMessage(`Permissions reset to defaults for ${name}.`)
    } catch (error) { setError(error instanceof Error ? error.message : 'Could not remove permissions.') }
    finally { setBusy(false) }
  }
  return <dialog ref={dialog} className="video-backdrop" aria-labelledby="permissions-title" onCancel={event => { event.preventDefault(); onClose() }}>
    <section className="permissions-panel">
      <header className="video-header"><div><h2 id="permissions-title">Source permissions</h2><span>{source.name}</span></div><button aria-label="Close source permissions" onClick={onClose}><X size={21}/></button></header>
      <div className="permissions-content"><p>Grant access by username. Visibility, media access, and acquisition are separate permissions. Everyone else is denied by default.</p>
        {loading ? <p role="status">Loading permissions…</p> : <>
          <div className="grants-list">{permissions.length === 0 ? <p>No per-user permissions configured. Only you can access this source.</p> : permissions.map(permission => <article className="grant-row" key={permission.user_id}>
            <div><strong>{permission.username}{permission.username === ownerUsername ? ' (owner)' : ''}</strong><span>{permission.can_discover ? 'Visible' : 'Hidden'} · {permission.can_access_media ? 'Media allowed' : 'Media denied'} · Acquisition: {policyLabel(permission.acquisition_policy)}</span></div>
            <button disabled={busy} onClick={() => { setUsername(permission.username); setGrant({ can_discover: permission.can_discover, can_access_media: permission.can_access_media, acquisition_policy: permission.acquisition_policy }); setMessage('') }} aria-label={`Edit permissions for ${permission.username}`}>Edit</button>
            <button disabled={busy} onClick={() => { void revoke(permission.username) }} aria-label={`Remove permissions for ${permission.username}`}>Remove</button>
          </article>)}</div>
          <form className="grant-form" onSubmit={save}>
            <label htmlFor="grant-username">Account username</label><input id="grant-username" autoComplete="off" autoCapitalize="none" spellCheck={false} placeholder="Enter an existing username" minLength={3} maxLength={32} pattern="[A-Za-z0-9_-]{3,32}" required value={username} disabled={busy} onChange={event => { setUsername(event.target.value); setGrant(denied()); setMessage('') }}/>
            <label className="grant-toggle"><input type="checkbox" checked={effective.can_discover} disabled={busy || owner} onChange={event => setGrant(event.target.checked ? { ...grant, can_discover: true } : denied())}/><span><strong>Source visibility</strong><small>Show this source in the user's source list.</small></span></label>
            <label className="grant-toggle"><input type="checkbox" checked={effective.can_access_media} disabled={busy || owner || !effective.can_discover} onChange={event => setGrant({ ...grant, can_access_media: event.target.checked })}/><span><strong>Media access</strong><small>Browse, play, and read all supported files in this source.</small></span></label>
            <label htmlFor="grant-acquisition">Acquisition policy</label><select id="grant-acquisition" value={effective.acquisition_policy} disabled={busy || !effective.can_discover} onChange={event => setGrant({ ...grant, acquisition_policy: event.target.value as AcquisitionPolicy })}><option value="auto_deny">Auto-deny</option><option value="manual_approval">Manual approval</option><option value="auto_accept">Auto-accept</option></select>
            <small>Acquisition requests are not available yet. This setting is saved for the future request workflow and does not grant media access.</small>
            {owner && <small>You retain source visibility, media access, and management as the owner. You can configure your own acquisition policy here.</small>}
            <button className="connect-submit" disabled={busy} type="submit">{busy ? 'Saving…' : 'Save permissions'}</button>
          </form>
          <p className="grant-hint">Removing a grant restores default deny. Revocation applies to new requests; content already delivered cannot be recalled.</p>
        </>}
        {error && <p className="video-error" role="alert">{error}</p>}{message && <p className="permission-success" role="status">{message}</p>}
      </div>
    </section>
  </dialog>
}
