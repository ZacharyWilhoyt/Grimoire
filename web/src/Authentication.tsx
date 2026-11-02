import { useEffect, useState, type FormEvent, type ReactNode } from 'react'
import { apiFetch, setCsrfToken } from './api'

export type User = { id: string; username: string; is_admin: boolean }
type AccountSession = { user: User; csrf_token: string }
type Props = { children: (user: User, logout: () => void) => ReactNode }

export default function Authentication({ children }: Props) {
  const [user, setUser] = useState<User | null>(null)
  const [checking, setChecking] = useState(true)
  const [mode, setMode] = useState<'login' | 'register'>('login')
  const [username, setUsername] = useState('')
  const [password, setPassword] = useState('')
  const [confirm, setConfirm] = useState('')
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState('')
  const accept = (session: AccountSession) => { setCsrfToken(session.csrf_token); setUser(session.user); setPassword(''); setConfirm('') }

  useEffect(() => {
    let active = true
    fetch('/api/auth/me', { credentials: 'same-origin', cache: 'no-store' }).then(async response => {
      if (response.ok) { const session = await response.json() as AccountSession; if (active) accept(session) }
      else if (response.status !== 401) throw new Error('The account service is unavailable. Try again shortly.')
    }).catch(error => { if (active) setError(error instanceof Error ? error.message : 'Could not reach Grimoire.') }).finally(() => { if (active) setChecking(false) })
    const expired = () => { setCsrfToken(''); setUser(null); setError('Your session ended. Please sign in again.') }
    window.addEventListener('grimoire-session-expired', expired)
    return () => { active = false; window.removeEventListener('grimoire-session-expired', expired) }
  }, [])

  const submit = async (event: FormEvent) => {
    event.preventDefault()
    if (mode === 'register' && password !== confirm) { setError('Passwords do not match.'); return }
    setBusy(true); setError('')
    try {
      const response = await fetch(`/api/auth/${mode}`, { method: 'POST', headers: { 'Content-Type': 'application/json' }, credentials: 'same-origin', body: JSON.stringify({ username, password }) })
      const data = await response.json()
      if (!response.ok) throw new Error(data.error ?? 'Could not sign in.')
      accept(data as AccountSession)
    } catch (error) { setError(error instanceof Error ? error.message : 'Could not reach Grimoire.') }
    finally { setBusy(false) }
  }
  const logout = async () => {
    setError('')
    try {
      const response = await apiFetch('/api/auth/logout', { method: 'POST' })
      if (!response.ok && response.status !== 401) throw new Error('Sign out failed. Please try again.')
      setCsrfToken(''); setUser(null); setPassword(''); setConfirm('')
    } catch (error) { setError(error instanceof Error ? error.message : 'Sign out failed.') }
  }
  if (checking) return <main className="auth-screen"><p role="status">Connecting to Grimoire…</p></main>
  if (user) return <>{error && <div className="account-error" role="alert">{error}<button onClick={() => setError('')}>Dismiss</button></div>}{children(user, () => { void logout() })}</>
  return <main className="auth-screen"><form className="auth-card" onSubmit={submit}>
    <a className="brand" href="/"><img className="brand-logo" src="/logo.svg" alt=""/><span>grimoire<span className="brand-dot">.</span></span></a>
    <div className="section-kicker">YOUR MEDIA, TOGETHER</div>
    <h1>{mode === 'login' ? 'Welcome back.' : 'Make yourself at home.'}</h1>
    <p>{mode === 'login' ? 'Sign in to your media space.' : 'Create your account. Source and media access are granted separately.'}</p>
    <label htmlFor="auth-username">Username</label><input id="auth-username" name="username" autoComplete="username" autoCapitalize="none" spellCheck={false} minLength={3} maxLength={32} pattern="[A-Za-z0-9_-]{3,32}" required value={username} onChange={event => setUsername(event.target.value)} disabled={busy}/>
    {mode === 'register' && <small>3–32 letters, digits, underscores, or hyphens.</small>}
    <label htmlFor="auth-password">Password</label><input id="auth-password" name="password" type="password" autoComplete={mode === 'login' ? 'current-password' : 'new-password'} minLength={mode === 'register' ? 15 : undefined} maxLength={128} required value={password} onChange={event => setPassword(event.target.value)} disabled={busy}/>
    {mode === 'register' && <><small>Use 15–128 characters. A long, unique passphrase works well.</small><label htmlFor="auth-confirm">Confirm password</label><input id="auth-confirm" type="password" autoComplete="new-password" required value={confirm} onChange={event => setConfirm(event.target.value)} disabled={busy}/></>}
    {error && <p className="auth-error" role="alert">{error}</p>}
    <button type="submit" className="connect-submit" disabled={busy}>{busy ? 'Please wait…' : mode === 'login' ? 'Sign in' : 'Create account'}</button>
    <button className="auth-switch" type="button" disabled={busy} onClick={() => { setMode(mode === 'login' ? 'register' : 'login'); setPassword(''); setConfirm(''); setError('') }}>{mode === 'login' ? 'New here? Create an account' : 'Already have an account? Sign in'}</button>
  </form></main>
}
