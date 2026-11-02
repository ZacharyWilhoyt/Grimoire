// @vitest-environment jsdom
import { act } from 'react'
import { createRoot } from 'react-dom/client'
import { expect, it, vi } from 'vitest'
import ActiveSessions from './ActiveSessions'
const api = vi.hoisted(() => vi.fn())
vi.mock('./api', () => ({ apiFetch: api }))
it('shows current/other session activity and reloads after account revocation', async () => {
  vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true)
  const current = { created_at: 100, last_seen: 200, is_current: true }
  api.mockReset().mockResolvedValueOnce({ ok: true, json: async () => [current, { ...current, is_current: false }] }).mockResolvedValueOnce({ ok: true, json: async () => [current] })
  const host = document.createElement('div'), root = createRoot(host)
  try {
    await act(async () => root.render(<ActiveSessions refreshKey={0}/>))
    expect(host.textContent).toContain('Current session'); expect(host.textContent).toContain('Other session')
    expect(host.querySelector('button')?.type).toBe('button')
    const oldSignal = api.mock.calls[0][1].signal as AbortSignal
    await act(async () => root.render(<ActiveSessions refreshKey={1}/>))
    expect(oldSignal.aborted).toBe(true)
    expect(host.textContent).not.toContain('Other session'); expect(host.textContent).toContain('Current session')
    expect(api).toHaveBeenCalledTimes(2)
  } finally { await act(async () => root.unmount()); vi.unstubAllGlobals() }
})
it('reports an unavailable session list and retries without claiming it is empty', async () => {
  vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true)
  api.mockReset().mockRejectedValueOnce(new Error('Connection unavailable')).mockResolvedValueOnce({ ok: true, json: async () => [{ created_at: 100, last_seen: 200, is_current: true }] })
  const host = document.createElement('div'), root = createRoot(host)
  try {
    await act(async () => root.render(<ActiveSessions refreshKey={0}/>))
    expect(host.querySelector('[role="alert"]')?.textContent).toContain('Connection unavailable')
    expect(host.querySelector('ul')).toBeNull()
    await act(async () => host.querySelector('button')!.click())
    expect(host.textContent).toContain('Current session'); expect(host.querySelector('[role="alert"]')).toBeNull()
  } finally { await act(async () => root.unmount()); vi.unstubAllGlobals() }
})
