import { useCallback, useEffect, useRef, useState } from 'react'
import { ProgressSession } from './progress'
import type { ResumePoint } from './playbackRouting'
export function useMediaProgress<T extends HTMLMediaElement>(sourceId: string, path: string, resourceKey = '', restorePosition = true, resume?: ResumePoint) {
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
    if (resume?.volume!==undefined) element.volume=resume.volume
    if (resume?.muted!==undefined) element.muted=resume.muted
    let active = true, loaded = false, restored = false, savedPosition = 0, lastSave = 0, interacted = false, pausedEarly = false
    setError('')
    const session = new ProgressSession(sourceId, path, message => { if (active) setError(message) })
    const restore = () => {
      if (!active || !loaded || restored || element.readyState < 1) return
      restored = true
      const compatible = resume && (resume.exact || resume.total !== undefined &&
        Math.abs(resume.total - element.duration) <= 1)
      const position = compatible ? resume.position : restorePosition ? savedPosition : 0
      if (!interacted && position > 0 && Number.isFinite(element.duration) && position < element.duration) element.currentTime = position
      if (!pausedEarly && resume?.paused !== true) void element.play().catch(() => { /* Browser autoplay policy; native Play remains available. */ })
    }
    let played=0,sampledAt=performance.now(),sampledPosition=element.currentTime,wasPlaying=!element.paused
    const sample=()=>{
      const now=performance.now(),wall=(now-sampledAt)/1000,delta=element.currentTime-sampledPosition
      if(element.tagName==='VIDEO'&&wasPlaying&&!element.seeking&&wall>0&&wall<=15&&delta>0&&delta<=wall*Math.max(1,element.playbackRate)+.5)played+=Math.min(wall,delta/Math.max(.25,element.playbackRate))
      sampledAt=now;sampledPosition=element.currentTime;wasPlaying=!element.paused
    }
    const resetSample=()=>{sampledAt=performance.now();sampledPosition=element.currentTime;wasPlaying=!element.paused}
    element.addEventListener('seeking',resetSample);element.addEventListener('seeked',resetSample);element.addEventListener('play',resetSample)
    const save = () => {
      if (!restored || !Number.isFinite(element.currentTime) || !Number.isFinite(element.duration) || element.duration <= 0) return
      sample()
      session.save({ ...(element.tagName==='VIDEO'?{played_seconds:Math.min(60,played),...(element.ended?{completed:true}:{})}:{}), position: Math.min(element.currentTime, element.duration), total: element.duration, locator: null })
      played=0
      lastSave = Date.now()
    }
    const paused = () => { if (!restored) pausedEarly = true; save() }
    const earlyInteraction = () => { if (!restored) interacted = true }
    element.addEventListener('seeking', earlyInteraction)
    const tick = () => { sample(); if (!element.paused && Date.now() - lastSave >= 10000) save() }
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
      element.removeEventListener('seeking',resetSample);element.removeEventListener('seeked',resetSample);element.removeEventListener('play',resetSample)
      element.removeEventListener('seeking', earlyInteraction); element.removeEventListener('loadedmetadata', restore); element.removeEventListener('timeupdate', tick); element.removeEventListener('pause', paused); element.removeEventListener('seeked', save); element.removeEventListener('ended', save)
      document.removeEventListener('visibilitychange', hidden); window.removeEventListener('pagehide', save)
    }
  }, [sourceId, path, retryKey, resourceKey, restorePosition, resume])
  return { media, progressError: error, retry, retryKey }
}
