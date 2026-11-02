// @vitest-environment jsdom
import { act } from 'react'
import { createRoot } from 'react-dom/client'
import { expect, it, vi } from 'vitest'
import VideoSubtitles from './VideoSubtitles'
const renderer = vi.hoisted(() => ({ options: vi.fn(), destroy: vi.fn(async () => {}), resize: vi.fn(async () => {}) }))
vi.mock('jassub', () => ({ default: class {
  ready = Promise.resolve(); timeOffset = 0; destroy = renderer.destroy; resize = renderer.resize
  constructor(options: unknown) { renderer.options(options) }
} }))
it('keeps subtitles off until selected and preserves ASS script and font bytes for libass', async () => {
  vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true)
  const script = '[Script Info]\n[Events]\nDialogue: {\\pos(20,30)\\k100}Text'
  vi.stubGlobal('fetch', vi.fn(async (url: string) => url.includes('/subtitles?')
    ? { ok: true, json: async () => ({ tracks: [{ path: 'embedded:2', label: 'Styled', format: 'ass' }], fonts: [3], truncated: false, warning: null }) }
    : url.includes('subtitle-font') ? { ok: true, arrayBuffer: async () => new Uint8Array([1, 2, 3]).buffer }
    : { ok: true, text: async () => script }))
  const host = document.createElement('div'), root = createRoot(host), video = document.createElement('video')
  renderer.options.mockClear(); renderer.destroy.mockClear()
  try {
    await act(async () => root.render(<VideoSubtitles video={video} sourceId="source" path="clip.mkv"/>))
    expect(renderer.options).not.toHaveBeenCalled()
    const select = host.querySelector('select')!
    await act(async () => { select.value = 'embedded:2'; select.dispatchEvent(new Event('change', { bubbles: true })) })
    expect(renderer.options).toHaveBeenCalledWith(expect.objectContaining({ video, subContent: script, fonts: [new Uint8Array([1, 2, 3])], queryFonts: false }))
    await act(async () => { select.value = ''; select.dispatchEvent(new Event('change', { bubbles: true })) })
    expect(renderer.destroy).toHaveBeenCalledTimes(1)
  } finally { await act(async () => root.unmount()); vi.unstubAllGlobals() }
})
