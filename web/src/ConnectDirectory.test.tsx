// @vitest-environment jsdom
import { act } from 'react'
import { createRoot } from 'react-dom/client'
import { expect, it, vi } from 'vitest'
import ConnectDirectory from './ConnectDirectory'
it('uses native modal lifecycle, delegates submit and supports Escape/backdrop closing', async () => {
  vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true)
  const show = vi.fn(), close = vi.fn()
  Object.defineProperty(HTMLDialogElement.prototype, 'showModal', { configurable: true, value: show })
  Object.defineProperty(HTMLDialogElement.prototype, 'close', { configurable: true, value: close })
  const host = document.createElement('div'), root = createRoot(host), dismissed = vi.fn(), submit = vi.fn()
  const opener = document.createElement('button'); document.body.append(opener, host); opener.focus()
  try {
    await act(async () => root.render(<ConnectDirectory path="/media" error="Example error" busy={false} onPath={() => {}} onClose={dismissed} onSubmit={event => { event.preventDefault(); submit() }}/>))
    expect(show).toHaveBeenCalledOnce()
    expect(document.activeElement).toBe(host.querySelector('#directory-path'))
    expect(host.querySelector('dialog')?.getAttribute('aria-labelledby')).toBe('connect-directory-title')
    expect(host.querySelector('[role="alert"]')?.textContent).toBe('Example error')
    await act(async () => host.querySelector('form')!.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true })))
    expect(submit).toHaveBeenCalledOnce()
    await act(async () => host.querySelector('form')!.click())
    expect(dismissed).not.toHaveBeenCalled()
    const cancel = new Event('cancel', { cancelable: true })
    await act(async () => host.querySelector('dialog')!.dispatchEvent(cancel))
    expect(cancel.defaultPrevented).toBe(true)
    expect(dismissed).toHaveBeenCalledOnce()
    await act(async () => host.querySelector('dialog')!.click())
    expect(dismissed).toHaveBeenCalledTimes(2)
  } finally { await act(async () => root.unmount()); expect(close).toHaveBeenCalledOnce(); expect(document.activeElement).toBe(opener); host.remove(); opener.remove(); vi.unstubAllGlobals() }
})
