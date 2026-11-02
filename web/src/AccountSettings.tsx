import ActiveSessions from './ActiveSessions'
import { useModalDialog } from './useModalDialog'
import { useState, type FormEvent } from 'react'
import { X } from 'lucide-react'
import { apiFetch, setCsrfToken } from './api'

export default function AccountSettings({ onClose }: { onClose: () => void }) {
  const dialog = useModalDialog()
  const [sessionsRefresh, setSessionsRefresh] = useState(0)
  const [current, setCurrent] = useState('')
  const [password, setPassword] = useState('')
  const [confirm, setConfirm] = useState('')
  const [busy, setBusy] = useState(false)
  const [message, setMessage] = useState('')
  const [error, setError] = useState('')
  const submit = async (event: FormEvent) => {
    event.preventDefault(); setError(''); setMessage('')
    if (password !== confirm) { setError('Passwords do not match.'); return }
    setBusy(true)
    try {
      const response = await apiFetch('/api/auth/password', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ current_password: current, new_password: password }) })
      const data = await response.json()
      if (!response.ok) throw new Error(data.error ?? 'Could not change your password.')
      setCsrfToken(data.csrf_token as string)
      setSessionsRefresh(value => value + 1)
      setCurrent(''); setPassword(''); setConfirm(''); setMessage('Password changed. Your other sessions have been signed out.')
    } catch (error) { setError(error instanceof Error ? error.message : 'Could not change your password.') }
    finally { setBusy(false) }
  }
  const logoutOthers = async () => {
    setError(''); setMessage('')
    if (!current) { setError('Enter your current password to sign out other sessions.'); return }
    setBusy(true)
    try {
      const response = await apiFetch('/api/auth/logout-others', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ current_password: current }) })
      const data = await response.json()
      if (!response.ok) throw new Error(data.error ?? 'Could not sign out other sessions.')
      setSessionsRefresh(value => value + 1)
      setCurrent(''); setMessage(`Signed out ${data.revoked} other ${data.revoked === 1 ? 'session' : 'sessions'}. This session remains signed in.`)
    } catch (error) { setError(error instanceof Error ? error.message : 'Could not sign out other sessions.') }
    finally { setBusy(false) }
  }
  return <dialog ref={dialog} className="video-backdrop" aria-labelledby="account-settings-title" onCancel={event => { event.preventDefault(); onClose() }}><form className="auth-card" onSubmit={submit}>
    <div className="settings-title"><h1 id="account-settings-title">Your account</h1><button type="button" aria-label="Close account settings" onClick={onClose}><X size={20}/></button></div>
    <p>Review active sessions, change your password, or sign out other sessions.</p>
    <ActiveSessions refreshKey={sessionsRefresh}/>
    <label htmlFor="current-password">Current password</label><input id="current-password" autoComplete="current-password" type="password" maxLength={128} required value={current} onChange={event => setCurrent(event.target.value)} disabled={busy}/>
    <label htmlFor="new-password">New password</label><input id="new-password" autoComplete="new-password" type="password" minLength={15} maxLength={128} required value={password} onChange={event => setPassword(event.target.value)} disabled={busy}/>
    <small>Use a unique password of 15–128 characters.</small>
    <label htmlFor="confirm-new-password">Confirm new password</label><input id="confirm-new-password" autoComplete="new-password" type="password" required value={confirm} onChange={event => setConfirm(event.target.value)} disabled={busy}/>
    {error && <p className="auth-error" role="alert">{error}</p>}{message && <p className="account-success" role="status">{message}</p>}
    <button className="track-play" type="button" disabled={busy} onClick={() => { void logoutOthers() }}>Sign out other sessions</button>
    <small>Uses your current password above. A new password is not required for this action.</small>
    <button className="connect-submit" type="submit" disabled={busy}>{busy ? 'Updating…' : 'Change password'}</button>
  </form></dialog>
}
