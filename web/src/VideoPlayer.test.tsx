// @vitest-environment jsdom
import { act } from 'react'
import { createRoot } from 'react-dom/client'
import { expect, it, vi } from 'vitest'
import VideoPlayer from './VideoPlayer'
const progress = vi.hoisted(() => ({ media: { current: null }, progressError: '', retry: vi.fn(), retryKey: 0 }))
vi.mock('./useMediaProgress', () => ({ useMediaProgress: () => progress }))
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
