// @vitest-environment jsdom
import { act } from 'react'
import { createRoot } from 'react-dom/client'
import { expect, it, vi } from 'vitest'
import VideoPlayer from './VideoPlayer'
const progress = vi.hoisted(() => ({ media: { current: null }, progressError: '', retry: vi.fn(), retryKey: 0 }))
vi.mock('./useMediaProgress', () => ({ useMediaProgress: () => progress }))
vi.mock('./WatchSidebar', () => ({ default: () => <aside><textarea disabled/></aside> }))
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
    const button = () => host.querySelector<HTMLButtonElement>('.watch-fullscreen')!
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
