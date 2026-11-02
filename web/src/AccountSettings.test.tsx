// @vitest-environment jsdom
import { act } from 'react'
import { createRoot } from 'react-dom/client'
import { expect, it, vi } from 'vitest'
import AccountSettings from './AccountSettings'
const controls = vi.hoisted(() => ({ api: vi.fn(), csrf: vi.fn() }))
vi.mock('./api', () => ({ apiFetch: controls.api, setCsrfToken: controls.csrf }))
for (const action of ['password', 'logout-others']) it(`refreshes the private session view after ${action} succeeds`, async () => {
  vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true)
  const show = vi.fn()
  Object.defineProperty(HTMLDialogElement.prototype, 'showModal', { configurable: true, value: show })
  Object.defineProperty(HTMLDialogElement.prototype, 'close', { configurable: true, value: vi.fn() })
  let others = true
  const current = { created_at: 100, last_seen: 200, is_current: true }
  controls.csrf.mockClear()
  controls.api.mockReset().mockImplementation(async (url: string) => {
    if (url === '/api/auth/sessions') return { ok: true, json: async () => others ? [current, { ...current, is_current: false }] : [current] }
    if (url === `/api/auth/${action}`) { others = false; return { ok: true, json: async () => ({ csrf_token: 'rotated-csrf', revoked: 1 }) } }
    throw new Error('Unexpected request')
  })
  const host = document.createElement('div'), root = createRoot(host)
  document.body.append(host)
  const fill = (id: string, value: string) => {
    const input = host.querySelector<HTMLInputElement>(`#${id}`)!
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!.call(input, value)
    input.dispatchEvent(new Event('input', { bubbles: true }))
  }
  try {
    await act(async () => root.render(<AccountSettings onClose={() => {}}/>))
    expect(host.textContent).toContain('Other session')
    await act(async () => {
      fill('current-password', 'current account passphrase')
      if (action === 'password') { fill('new-password', 'a new account passphrase'); fill('confirm-new-password', 'a new account passphrase') }
    })
    await act(async () => {
      if (action === 'password') host.querySelector('form')!.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true }))
      else [...host.querySelectorAll('button')].find(button => button.textContent === 'Sign out other sessions')!.click()
    })
    expect(host.textContent).not.toContain('Other session')
    expect(host.textContent).toContain('Current session')
    expect(host.querySelector<HTMLInputElement>('#current-password')!.value).toBe('')
    expect(controls.api.mock.calls.filter(([url]) => url === '/api/auth/sessions')).toHaveLength(2)
    expect(show).toHaveBeenCalledOnce()
    if (action === 'password') expect(controls.csrf).toHaveBeenCalledWith('rotated-csrf')
    else expect(controls.csrf).not.toHaveBeenCalled()
  } finally { await act(async () => root.unmount()); host.remove(); vi.unstubAllGlobals() }
})
