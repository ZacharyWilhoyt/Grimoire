import { useCallback, useEffect, useRef, useState } from 'react'
import { ProgressSession } from './progress'
export function useMediaProgress<T extends HTMLMediaElement>(sourceId: string, path: string, resourceKey = '', restorePosition = true) {
  const media = useRef<T | null>(null)
  const [error, setError] = useState('')
  const [retryKey, setRetryKey] = useState(0)
  const previousFlush = useRef<Promise<void>>(Promise.resolve())
  const retrySettings = useRef<{ volume: number; muted: boolean; playbackRate: number } | null>(null)
  const retry = useCallback(() => {
    const element = media.current
    if (element) retrySettings.current = { volume: element.volume, muted: element.muted, playbackRate: element.playbackRate }
    setRetryKey(value => value + 1)
  }, [])
  useEffect(() => {
    const element = media.current
    if (!element) return
    if (retrySettings.current) { Object.assign(element, retrySettings.current); retrySettings.current = null }
    let active = true, loaded = false, restored = false, savedPosition = 0, lastSave = 0, interacted = false, pausedEarly = false
    setError('')
    const session = new ProgressSession(sourceId, path, message => { if (active) setError(message) })
    const restore = () => {
      if (!active || !loaded || restored || element.readyState < 1) return
      restored = true
      if (restorePosition && !interacted && savedPosition > 0 && Number.isFinite(element.duration) && savedPosition < element.duration) element.currentTime = savedPosition
      if (!pausedEarly) void element.play().catch(() => { /* Browser autoplay policy; native Play remains available. */ })
    }
    const save = () => {
      if (!restored || !Number.isFinite(element.currentTime) || !Number.isFinite(element.duration) || element.duration <= 0) return
      session.save({ position: Math.min(element.currentTime, element.duration), total: element.duration, locator: null })
      lastSave = Date.now()
    }
    const paused = () => { if (!restored) pausedEarly = true; save() }
    const earlyInteraction = () => { if (!restored) interacted = true }
    element.addEventListener('seeking', earlyInteraction)
    const tick = () => { if (!element.paused && Date.now() - lastSave >= 10000) save() }
    const hidden = () => { if (document.visibilityState === 'hidden') save() }
    element.addEventListener('loadedmetadata', restore)
    element.addEventListener('timeupdate', tick)
    element.addEventListener('pause', paused)
    element.addEventListener('seeked', save)
    element.addEventListener('ended', save)
    document.addEventListener('visibilitychange', hidden)
    window.addEventListener('pagehide', save)
    void previousFlush.current.then(() => active ? session.load() : null).then(progress => { if (active) { savedPosition = progress?.position ?? 0; loaded = true; restore() } })
    return () => {
      save(); previousFlush.current = session.flush(); active = false
      retrySettings.current = { volume: element.volume, muted: element.muted, playbackRate: element.playbackRate }
      element.removeEventListener('seeking', earlyInteraction); element.removeEventListener('loadedmetadata', restore); element.removeEventListener('timeupdate', tick); element.removeEventListener('pause', paused); element.removeEventListener('seeked', save); element.removeEventListener('ended', save)
      document.removeEventListener('visibilitychange', hidden); window.removeEventListener('pagehide', save)
    }
  }, [sourceId, path, retryKey, resourceKey, restorePosition])
  return { media, progressError: error, retry, retryKey }
}
