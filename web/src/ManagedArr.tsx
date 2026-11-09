import { useState, type FormEvent } from 'react'
import type { ArrConnection } from './ArrConnections'

export type ManagedInstance = {
  id: string
  status: string
  stage?: string | null
  error?: string | null
  local_port?: number | null
  version?: string | null
}

type CreateFields = {
  id: string
  name: string
  kind: ArrConnection['kind']
  source_subpath: string
  username: string
  password: string
  writable_media: boolean
}

export function ManagedArrForm({ id, retry, busy, onSubmit, onCancel }: {
  id: string
  retry?: ArrConnection
  busy: boolean
  onSubmit: (fields: CreateFields) => Promise<void>
  onCancel: () => void
}) {
  const [name, setName] = useState(retry?.name ?? '')
  const [kind, setKind] = useState<ArrConnection['kind']>(retry?.kind ?? 'sonarr')
  const [folder, setFolder] = useState(retry?.source_subpath ?? '')
  const [writable, setWritable] = useState(retry?.writable_media ?? false)
  const [username, setUsername] = useState('grimoire')
  const [password, setPassword] = useState('')

  function submit(event: FormEvent) {
    event.preventDefault()
    void onSubmit({ id, name, kind, source_subpath: folder, username, password, writable_media: writable })
      .finally(() => setPassword(''))
  }

  return <form className="metadata-form" onSubmit={submit}>
    <h3>{retry ? 'Retry managed instance setup' : 'Create managed Arr instance'}</h3>
    <label>Instance name<input required maxLength={80} placeholder="Anime Sonarr" value={name}
      disabled={busy || !!retry} onChange={event => setName(event.target.value)}/></label>
    <label>Arr app<select value={kind} disabled={busy || !!retry}
      onChange={event => setKind(event.target.value as ArrConnection['kind'])}>
      <option value="sonarr">Sonarr — series / anime</option>
      <option value="radarr">Radarr — movies</option>
      <option value="lidarr">Lidarr — music</option>
    </select></label>
    <label>Folder inside this source<input maxLength={4096} value={folder} disabled={busy || !!retry}
      placeholder="Leave blank for the source root" onChange={event => setFolder(event.target.value)}/></label>
    <small>The selected folder appears as /media inside this Arr instance.</small>
    <label className="grant-toggle"><input type="checkbox" checked={writable} disabled={busy || !!retry}
      onChange={event => setWritable(event.target.checked)}/> Allow this Arr instance to modify media files</label>
    <small>{writable ? 'Arr can write to this folder where host permissions allow it.' : 'Media is mounted read-only.'}</small>
    <label>Arr login username<input required minLength={3} maxLength={64} pattern="[A-Za-z0-9_\-]+"
      autoComplete="username" value={username} disabled={busy} onChange={event => setUsername(event.target.value)}/></label>
    <label>Arr login password<input type="password" required minLength={12} maxLength={128} autoComplete="new-password"
      value={password} disabled={busy} onChange={event => setPassword(event.target.value)}/></label>
    <small>Use these credentials in the Arr setup UI. Grimoire generates and saves the separate API key.</small>
    <div className="arr-buttons">
      <button className="track-play" type="submit" disabled={busy}>{retry ? 'Retry setup' : 'Create instance'}</button>
      <button className="track-play" type="button" disabled={busy} onClick={onCancel}>Cancel</button>
    </div>
  </form>
}

export function ManagedArrControls({ instance, busy, onAction, onRetry, onClear }: {
  instance?: ManagedInstance
  busy: boolean
  onAction: (action: 'start' | 'stop' | 'restart' | 'remove') => void
  onRetry: () => void
  onClear: () => void
}) {
  const [confirm, setConfirm] = useState(false)
  const status = instance?.status ?? 'pending setup'
  const working = ['provisioning', 'changing', 'removing'].includes(status)
  return <div className="managed-arr-controls">
    <span role="status">{working ? instance?.stage ?? status : status}{instance?.version ? ` · ${instance.version}` : ''}</span>
    {instance?.error && <small role="alert">{instance.error}</small>}
    <div className="arr-buttons">
      {status === 'running' && instance?.local_port && <a className="track-play" href={`http://127.0.0.1:${instance.local_port}/`}
        target="_blank" rel="noreferrer" title="Open on the Grimoire server">Open local Arr UI</a>}
      {status === 'running' && <>
        <button className="track-play" disabled={busy} onClick={() => onAction('stop')}>Stop</button>
        <button className="track-play" disabled={busy} onClick={() => onAction('restart')}>Restart</button>
      </>}
      {status === 'stopped' && <button className="track-play" disabled={busy} onClick={() => onAction('start')}>Start</button>}
      {['failed', 'pending setup'].includes(status) && <button className="track-play" disabled={busy} onClick={onRetry}>Retry setup</button>}
      {!working && instance && status !== 'removed' && <button className="disconnect-button" disabled={busy}
        onClick={() => { if (confirm) { onAction('remove'); setConfirm(false) } else { setConfirm(true) } }}>
        {confirm ? 'Confirm container removal' : 'Remove container'}
      </button>}
      {confirm && <button className="track-play" onClick={() => setConfirm(false)}>Cancel</button>}
      {(status === 'removed' || !instance) && <button className="disconnect-button" disabled={busy} onClick={onClear}>Remove connection</button>}
    </div>
  </div>
}
