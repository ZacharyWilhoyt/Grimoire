import { useEffect, useRef, useState } from 'react'
import { ProgressSession } from './progress'
export function useMediaProgress<T extends HTMLMediaElement>(sourceId: string, path: string) {
  const media = useRef<T | null>(null)
  const [error, setError] = useState('')
  useEffect(() => {
    const element = media.current
    if (!element) return
    let active = true, loaded = false, restored = false, savedPosition = 0, lastSave = 0, interacted = false
    setError('')
    const session = new ProgressSession(sourceId, path, message => { if (active) setError(message) })
    const restore = () => {
      if (!active || !loaded || restored || element.readyState < 1) return
      restored = true
      if (!interacted && savedPosition > 0 && Number.isFinite(element.duration) && savedPosition < element.duration) element.currentTime = savedPosition
      void element.play().catch(() => { /* Browser autoplay policy; native Play remains available. */ })
    }
    const save = () => {
      if (!restored || !Number.isFinite(element.currentTime) || !Number.isFinite(element.duration) || element.duration <= 0) return
      session.save({ position: Math.min(element.currentTime, element.duration), total: element.duration, locator: null })
      lastSave = Date.now()
    }
    const earlyInteraction = () => { if (!restored) interacted = true }
    element.addEventListener('seeking', earlyInteraction)
    const tick = () => { if (!element.paused && Date.now() - lastSave >= 10000) save() }
    const hidden = () => { if (document.visibilityState === 'hidden') save() }
    element.addEventListener('loadedmetadata', restore)
    element.addEventListener('timeupdate', tick)
    element.addEventListener('pause', save)
    element.addEventListener('seeked', save)
    element.addEventListener('ended', save)
    document.addEventListener('visibilitychange', hidden)
    window.addEventListener('pagehide', save)
    void session.load().then(progress => { if (active) { savedPosition = progress?.position ?? 0; loaded = true; restore() } })
    return () => {
      save(); active = false
      element.removeEventListener('seeking', earlyInteraction); element.removeEventListener('loadedmetadata', restore); element.removeEventListener('timeupdate', tick); element.removeEventListener('pause', save); element.removeEventListener('seeked', save); element.removeEventListener('ended', save)
      document.removeEventListener('visibilitychange', hidden); window.removeEventListener('pagehide', save)
    }
  }, [sourceId, path])
  return { media, progressError: error }
}
