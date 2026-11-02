// @vitest-environment jsdom
import { act, useRef } from 'react'
import { createRoot } from 'react-dom/client'
import { expect, it, vi } from 'vitest'
import { useModalDialog } from './useModalDialog'
function Example({ label }: { label: string }) {
  const input = useRef<HTMLInputElement>(null), dialog = useModalDialog(input)
  return <dialog ref={dialog}><input ref={input} aria-label="Edit"/><button>{label}</button></dialog>
}
for (const removeOpener of [false, true]) it(`keeps focus during content refresh and ${removeOpener ? 'ignores a disconnected opener' : 'restores the opener on close'}`, async () => {
  vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true)
  const show = vi.fn(function(this: HTMLDialogElement) { this.setAttribute('open', '') })
  const close = vi.fn(function(this: HTMLDialogElement) { this.removeAttribute('open') })
  Object.defineProperty(HTMLDialogElement.prototype, 'showModal', { configurable: true, value: show })
  Object.defineProperty(HTMLDialogElement.prototype, 'close', { configurable: true, value: close })
  const host = document.createElement('div'), root = createRoot(host), opener = document.createElement('button')
  document.body.append(opener, host); opener.focus()
  const focus = vi.spyOn(opener, 'focus')
  let unmounted = false
  try {
    await act(async () => root.render(<Example label="First"/>))
    const dialog = host.querySelector('dialog')!, input = host.querySelector('input')!, button = host.querySelector('button')!
    expect(document.activeElement).toBe(input)
    input.value = 'Unsubmitted edit'; button.focus()
    await act(async () => root.render(<Example label="Refreshed"/>))
    expect(show).toHaveBeenCalledOnce(); expect(close).not.toHaveBeenCalled()
    expect(document.activeElement).toBe(button); expect(input.value).toBe('Unsubmitted edit')
    if (removeOpener) opener.remove()
    await act(async () => root.unmount()); unmounted = true
    expect(close).toHaveBeenCalledOnce(); expect(dialog.open).toBe(false)
    expect(focus).toHaveBeenCalledTimes(removeOpener ? 0 : 1)
    if (!removeOpener) expect(document.activeElement).toBe(opener)
  } finally { if (!unmounted) await act(async () => root.unmount()); focus.mockRestore(); host.remove(); opener.remove(); vi.unstubAllGlobals() }
})
