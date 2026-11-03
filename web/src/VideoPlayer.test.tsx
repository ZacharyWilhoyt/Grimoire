// @vitest-environment jsdom
import { act } from 'react'
import { createRoot } from 'react-dom/client'
import { expect, it, vi } from 'vitest'
import VideoPlayer from './VideoPlayer'
const progress = vi.hoisted(() => ({ media: { current: null }, progressError: '', retry: vi.fn(), retryKey: 0 }))
vi.mock('./useMediaProgress', () => ({ useMediaProgress: () => progress }))
vi.mock('./WatchSidebar', () => ({ default: () => <aside><textarea disabled/></aside> }))
vi.mock('./TranscodeOptions', () => ({ default: () => null }))
vi.mock('./VideoAudio', () => ({ default: () => null }))
vi.mock('./VideoSubtitles', () => ({ default: () => null }))
it('offers an explicit retry after a video failure without closing the player', async () => {
  vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true)
  Object.defineProperty(HTMLDialogElement.prototype, 'showModal', { configurable: true, value: vi.fn() })
  Object.defineProperty(HTMLDialogElement.prototype, 'close', { configurable: true, value: vi.fn() })
  const host = document.createElement('div'), root = createRoot(host), close = vi.fn()
  progress.retry.mockClear()
  try {
    await act(async () => root.render(<VideoPlayer sourceId="source" sourceName="Test" file={{ name: 'Film', relative_path: 'film.mkv' }} onClose={close}/>))
    await act(async () => host.querySelector('video')!.dispatchEvent(new Event('error')))
    expect(host.textContent).toContain('This video could not be played')
    await act(async () => [...host.querySelectorAll('button')].find(button => button.textContent === 'Retry video')!.click())
    expect(progress.retry).toHaveBeenCalledOnce()
    expect(close).not.toHaveBeenCalled()
    expect(host.querySelector('[role="alert"]')).toBeNull()
  } finally { await act(async () => root.unmount()); vi.unstubAllGlobals() }
})

it('exits container fullscreen using the same visible button', async () => {
  vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true)
  const host = document.createElement('div'), root = createRoot(host)
  let fullscreen: Element | null = null
  Object.defineProperty(document, 'fullscreenElement', { configurable: true, get: () => fullscreen })
  const enter = vi.fn(async function(this: HTMLElement) { fullscreen = this; document.dispatchEvent(new Event('fullscreenchange')) })
  const exit = vi.fn(async () => { fullscreen = null; document.dispatchEvent(new Event('fullscreenchange')) })
  Object.defineProperty(HTMLElement.prototype, 'requestFullscreen', { configurable: true, value: enter })
  Object.defineProperty(document, 'exitFullscreen', { configurable: true, value: exit })
  try {
    await act(async () => root.render(<VideoPlayer sourceId="s" sourceName="Test" file={{name:'Film',relative_path:'film.mkv'}} onClose={() => {}}/>))
    const button = () => host.querySelector<HTMLButtonElement>('.toolbar-fullscreen')!
    expect(host.querySelector('.watch-stage')!.contains(button())).toBe(true)
    await act(async () => button().click())
    expect(button().getAttribute('aria-label')).toBe('Exit fullscreen')
    await act(async () => button().click())
    expect(exit).toHaveBeenCalledOnce()
    expect(enter).toHaveBeenCalledOnce()
    expect(button().getAttribute('aria-label')).toBe('Fullscreen with subtitles')
    expect(host.querySelector('dialog')).toBeNull()
    expect(host.querySelector('textarea')!.disabled).toBe(true)
  } finally { await act(async () => root.unmount()); vi.unstubAllGlobals(); delete (HTMLElement.prototype as Partial<HTMLElement>).requestFullscreen }
})
it('offers in-player option panels and toggles cozy view without losing video', async () => {
  vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true)
  const host=document.createElement('div'),root=createRoot(host)
  try {
    await act(async()=>root.render(<VideoPlayer sourceId="s" sourceName="Test" file={{name:'Film',relative_path:'film.mkv'}} onClose={()=>{}}/>))
    const video=host.querySelector('video')!
    const button=(label:string)=>host.querySelector<HTMLButtonElement>(`button[aria-label="${label}"]`)!
    for(const label of ['Subtitle options','Audio options','Settings','Episodes preview']) {
      await act(async()=>button(label).click())
      expect(button(label).getAttribute('aria-expanded')).toBe('true')
      expect(host.querySelector('.watch-stage')!.contains(button(label))).toBe(true)
    }
    await act(async()=>button('Cozy view').click())
    expect(host.querySelector('.watch-cozy')).not.toBeNull()
    expect(host.querySelector('video')).toBe(video)
    await act(async()=>button('Settings').click())
    const speed=host.querySelector<HTMLSelectElement>('[aria-label="Playback speed"]')!
    await act(async()=>{speed.value='1.5';speed.dispatchEvent(new Event('change',{bubbles:true}))})
    expect(video.playbackRate).toBe(1.5)
    expect(host.querySelector('[aria-label="Video filters"]')).toBeNull()
  } finally {await act(async()=>root.unmount());vi.unstubAllGlobals()}
})
it('uses one custom playback bar with seeking, volume and media event synchronization', async()=>{
  vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT',true)
  const host=document.createElement('div'),root=createRoot(host)
  try{
    await act(async()=>root.render(<VideoPlayer sourceId="s" sourceName="Test" file={{name:'Film',relative_path:'film.mkv'}} onClose={()=>{}}/>))
    const video=host.querySelector('video')!
    expect(video.controls).toBe(false)
    expect(host.querySelectorAll('[aria-label="Playback controls"]')).toHaveLength(1)
    Object.defineProperty(video,'duration',{configurable:true,value:120})
    await act(async()=>video.dispatchEvent(new Event('durationchange')))
    video.currentTime=35
    await act(async()=>video.dispatchEvent(new Event('timeupdate')))
    await act(async()=>host.querySelector<HTMLButtonElement>('[aria-label="Forward 10 seconds"]')!.click())
    expect(video.currentTime).toBe(45)
    await act(async()=>host.querySelector<HTMLButtonElement>('[aria-label="Mute"]')!.click())
    expect(video.muted).toBe(true)
    await act(async()=>video.dispatchEvent(new Event('volumechange')))
    expect(host.querySelector('[aria-label="Unmute"]')).not.toBeNull()
  }finally{await act(async()=>root.unmount());vi.unstubAllGlobals()}
})
it('hides controls only during idle playback and reveals them on pointer activity',async()=>{
 vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT',true);vi.useFakeTimers()
 const host=document.createElement('div'),root=createRoot(host)
 try{
 await act(async()=>root.render(<VideoPlayer sourceId="s" sourceName="Test" file={{name:'Film',relative_path:'film.mkv'}} onClose={()=>{}}/>))
 const video=host.querySelector('video')!,toolbar=host.querySelector('.video-toolbar')!,stage=host.querySelector('.watch-stage')!
 Object.defineProperty(video,'paused',{configurable:true,value:false})
 await act(async()=>video.dispatchEvent(new Event('play')))
 await act(async()=>vi.advanceTimersByTime(2600));expect(toolbar.classList.contains('toolbar-hidden')).toBe(true)
 await act(async()=>stage.dispatchEvent(new Event('pointermove')));expect(toolbar.classList.contains('toolbar-hidden')).toBe(false)
 await act(async()=>host.querySelector<HTMLButtonElement>('[aria-label="Settings"]')!.click())
 await act(async()=>vi.advanceTimersByTime(2600));expect(toolbar.classList.contains('toolbar-hidden')).toBe(false)
 await act(async()=>host.querySelector<HTMLButtonElement>('[aria-label="Cozy view"]')!.click());expect(host.querySelector('.watch-header')).toBeNull();expect(host.querySelector('[aria-label="Close video player"]')).not.toBeNull()
 }finally{await act(async()=>root.unmount());vi.useRealTimers();vi.unstubAllGlobals()}
})
