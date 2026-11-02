import { useEffect, useState } from 'react'
import { apiFetch } from './api'
type Session = { created_at: number; last_seen: number; is_current: boolean }
export default function ActiveSessions({ refreshKey }: { refreshKey: number }) {
  const [sessions, setSessions] = useState<Session[]>([])
  const [loading, setLoading] = useState(true), [error, setError] = useState(''), [attempt, setAttempt] = useState(0)
  useEffect(() => {
    const controller = new AbortController()
    setLoading(true); setError(''); setSessions([])
    const load = async () => {
      const response = await apiFetch('/api/auth/sessions', { signal: controller.signal })
      const data = await response.json()
      if (!response.ok || !Array.isArray(data)) throw new Error('Could not load active sessions.')
      if (!controller.signal.aborted) setSessions(data as Session[])
    }
    void load().catch(error => { if (!controller.signal.aborted) setError(error instanceof Error ? error.message : 'Could not load active sessions.') }).finally(() => { if (!controller.signal.aborted) setLoading(false) })
    return () => controller.abort()
  }, [refreshKey, attempt])
  return <section className="active-sessions" aria-labelledby="active-sessions-title" aria-busy={loading}>
    <div className="session-heading"><h2 id="active-sessions-title">Active sessions</h2><button type="button" className="track-play" aria-disabled={loading} onClick={() => { if (!loading) setAttempt(value => value + 1) }}>Refresh sessions</button></div>
    {loading ? <p role="status">Loading active sessions…</p> : error ? <p role="alert">{error} Refresh to try again.</p> : <>
      <div className="session-list" tabIndex={0} role="region" aria-label="Session activity"><ul>{sessions.map((session, index) => <li key={`${session.created_at}:${index}`}><strong>{session.is_current ? 'Current session' : 'Other session'}</strong><span>Signed in {new Date(session.created_at * 1000).toLocaleString()}</span><span>Last active {new Date(session.last_seen * 1000).toLocaleString()}</span></li>)}</ul></div>
      <small>Up to 10 active sessions are shown. Sessions expire after 30 minutes of inactivity or 12 hours.</small>
    </>}
  </section>
}
