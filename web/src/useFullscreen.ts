import { useCallback, useEffect, useState, type RefObject } from 'react'
export function useFullscreen(target: RefObject<HTMLElement | null>) {
  const [fullscreen, setFullscreen] = useState(false)
  const [error, setError] = useState('')
  useEffect(() => {
    const changed = () => setFullscreen(document.fullscreenElement === target.current)
    document.addEventListener('fullscreenchange', changed)
    return () => document.removeEventListener('fullscreenchange', changed)
  }, [target])
  const toggle = useCallback(async () => {
    setError('')
    try {
      if (document.fullscreenElement) await document.exitFullscreen()
      else if (target.current?.requestFullscreen) await target.current.requestFullscreen()
      else throw new Error('Fullscreen is unavailable in this browser.')
    } catch { setError('Fullscreen could not be changed. Try again or use Escape to exit.') }
  }, [target])
  return { fullscreen, toggle, error }
}
