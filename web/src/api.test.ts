// @vitest-environment jsdom
import { afterEach, expect, it, vi } from 'vitest'
import { apiFetch, setCsrfToken } from './api'
afterEach(() => { vi.unstubAllGlobals(); setCsrfToken('') })
it('keeps CSRF in memory and attaches it only to mutating same-origin requests', async () => {
  const fetch = vi.fn(async (_input: string, _init?: RequestInit) => ({ status: 200 } as Response))
  vi.stubGlobal('fetch', fetch)
  setCsrfToken('test-csrf')
  await apiFetch('/api/sources', { method: 'POST' })
  const init = fetch.mock.calls[0]?.[1] as RequestInit | undefined
  expect(new Headers(init?.headers).get('X-CSRF-Token')).toBe('test-csrf')
  expect(init?.credentials).toBe('same-origin')
  expect(init?.cache).toBe('no-store')
  await apiFetch('/api/sources')
  const getInit = fetch.mock.calls[1]?.[1] as RequestInit | undefined
  expect(new Headers(getInit?.headers).get('X-CSRF-Token')).toBeNull()
})
it('notifies the authentication gate when a session expires', async () => {
  const expired = vi.fn()
  window.addEventListener('grimoire-session-expired', expired)
  vi.stubGlobal('fetch', vi.fn(async () => ({ status: 401 } as Response)))
  await apiFetch('/api/sources')
  expect(expired).toHaveBeenCalledOnce()
  window.removeEventListener('grimoire-session-expired', expired)
})

it('refuses to send CSRF tokens outside the application origin', async () => {
  const fetch = vi.fn()
  vi.stubGlobal('fetch', fetch)
  setCsrfToken('secret-csrf')
  await expect(apiFetch('https://attacker.test/api/sources', { method: 'POST' })).rejects.toThrow('application origin')
  expect(fetch).not.toHaveBeenCalled()
})
