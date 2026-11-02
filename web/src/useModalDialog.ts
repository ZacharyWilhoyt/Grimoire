import { useEffect, useRef, type RefObject } from 'react'

// Keep modal lifetime separate from data-loading effects: refreshing a form or
// switching a reader item must not close/reopen the dialog or steal focus.
export function useModalDialog(initialFocus?: RefObject<HTMLElement | null>) {
  const dialog = useRef<HTMLDialogElement>(null)
  useEffect(() => {
    const element = dialog.current
    if (!element) return
    const opener = document.activeElement instanceof HTMLElement ? document.activeElement : null
    element.showModal()
    initialFocus?.current?.focus()
    return () => {
      // React may already have cleared the ref/removed the node at cleanup.
      element.close()
      if (opener?.isConnected) opener.focus({ preventScroll: true })
    }
  }, [initialFocus])
  return dialog
}
