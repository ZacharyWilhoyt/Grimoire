// @vitest-environment jsdom
import { act } from 'react'
import { createRoot } from 'react-dom/client'
import { expect, it, vi } from 'vitest'
import { useMediaProgress } from './useMediaProgress'
const session = vi.hoisted(() => ({ load: vi.fn(), save: vi.fn() }))
vi.mock('./progress', () => ({ ProgressSession: class { load = session.load; save = session.save } }))
function Player() { const { media } = useMediaProgress<HTMLVideoElement>('source', 'video.mp4'); return <video ref={media}/> }
for (const manualPause of [false, true]) it(`restores saved time and ${manualPause ? 'honors an early pause' : 'starts playback normally'}`, async () => {
  vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true)
  let resolve!: (value: { position: number }) => void
  session.load.mockReturnValue(new Promise(done => { resolve = done })); session.save.mockClear()
  const play = vi.spyOn(HTMLMediaElement.prototype, 'play').mockResolvedValue()
  const host = document.createElement('div'), root = createRoot(host)
  try {
    await act(async () => root.render(<Player/>))
    const video = host.querySelector('video')!
    Object.defineProperties(video, { duration: { value: 100 }, readyState: { value: 1 } })
    if (manualPause) video.dispatchEvent(new Event('pause'))
    await act(async () => { resolve({ position: 25 }) })
    expect(video.currentTime).toBe(25)
    expect(play).toHaveBeenCalledTimes(manualPause ? 0 : 1)
    video.currentTime = 30
    video.dispatchEvent(new Event('pause'))
    expect(session.save).toHaveBeenLastCalledWith({ position: 30, total: 100, locator: null })
  } finally { await act(async () => root.unmount()); play.mockRestore(); vi.unstubAllGlobals() }
})
it('preserves a manual seek while the saved position is loading', async () => {
  vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true)
  let resolve!: (value: { position: number }) => void
  session.load.mockReturnValue(new Promise(done => { resolve = done }))
  const play = vi.spyOn(HTMLMediaElement.prototype, 'play').mockResolvedValue()
  const host = document.createElement('div'), root = createRoot(host)
  try {
    await act(async () => root.render(<Player/>))
    const video = host.querySelector('video')!
    Object.defineProperties(video, { duration: { value: 100 }, readyState: { value: 1 } })
    video.currentTime = 50; video.dispatchEvent(new Event('seeking'))
    await act(async () => { resolve({ position: 25 }) })
    expect(video.currentTime).toBe(50)
  } finally { await act(async () => root.unmount()); play.mockRestore(); vi.unstubAllGlobals() }
})
