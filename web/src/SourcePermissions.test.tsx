// @vitest-environment jsdom
import { act } from 'react'
import { createRoot } from 'react-dom/client'
import { expect, it, vi } from 'vitest'
import SourcePermissions from './SourcePermissions'
const api = vi.hoisted(() => vi.fn())
vi.mock('./api', () => ({ apiFetch: api }))
it('does not equate a failed roster request with an empty grant list and recovers on retry', async () => {
  vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true)
  const show = vi.fn()
  Object.defineProperty(HTMLDialogElement.prototype, 'showModal', { configurable: true, value: show })
  Object.defineProperty(HTMLDialogElement.prototype, 'close', { configurable: true, value: vi.fn() })
  api.mockReset().mockRejectedValueOnce(new Error('Connection failed')).mockResolvedValueOnce({ ok: true, json: async () => [{ user_id: 'id', username: 'alice', can_discover: true, can_access_media: true, acquisition_policy: 'auto_deny' }] })
  const host = document.createElement('div'), root = createRoot(host)
  try {
    await act(async () => root.render(<SourcePermissions source={{ id: 'source', name: 'Test' }} ownerUsername="owner" onClose={() => {}} onSaved={() => {}}/>))
    expect(host.textContent).toContain('roster is unavailable')
    expect(host.textContent).not.toContain('Only you can access')
    expect(host.querySelector('form')).toBeNull()
    const retry = [...host.querySelectorAll('button')].find(button => button.textContent === 'Retry loading permissions')!
    await act(async () => retry.click())
    expect(host.textContent).toContain('alice')
    expect(show).toHaveBeenCalledOnce()
    expect(host.textContent).not.toContain('Connection failed')
    expect(host.querySelector('form')).not.toBeNull()
  } finally { await act(async () => root.unmount()); vi.unstubAllGlobals() }
})
it('reports a completed revocation even if refreshing the roster fails, and stops further edits', async () => {
  vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true)
  Object.defineProperty(HTMLDialogElement.prototype, 'showModal', { configurable: true, value: vi.fn() })
  Object.defineProperty(HTMLDialogElement.prototype, 'close', { configurable: true, value: vi.fn() })
  api.mockReset()
    .mockResolvedValueOnce({ ok: true, json: async () => [{ user_id: 'id', username: 'alice', can_discover: true, can_access_media: true, acquisition_policy: 'auto_deny' }] })
    .mockResolvedValueOnce({ ok: true })
    .mockRejectedValueOnce(new Error('Could not refresh permissions.'))
  const host = document.createElement('div'), root = createRoot(host)
  try {
    await act(async () => root.render(<SourcePermissions source={{ id: 'source', name: 'Test' }} ownerUsername="owner" onClose={() => {}} onSaved={() => {}}/>))
    await act(async () => host.querySelector<HTMLButtonElement>('button[aria-label="Remove permissions for alice"]')!.click())
    expect(api.mock.calls[1][1].method).toBe('DELETE')
    expect(host.textContent).toContain('Permissions reset to defaults for alice.')
    expect(host.textContent).toContain('Could not refresh permissions.')
    expect(host.querySelector('form')).toBeNull()
    expect(host.textContent).not.toContain('Only you can access')
  } finally { await act(async () => root.unmount()); vi.unstubAllGlobals() }
})
