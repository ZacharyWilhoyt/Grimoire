// @vitest-environment jsdom
import { act } from 'react'
import { createRoot } from 'react-dom/client'
import { expect, it, vi } from 'vitest'
import VideoSubtitles from './VideoSubtitles'
const renderer = vi.hoisted(() => ({ options: vi.fn(), destroy: vi.fn(async () => {}), resize: vi.fn(async () => {}) }))
vi.mock('jassub', () => ({ default: class {
  ready = Promise.resolve(); timeOffset = 0; destroy = renderer.destroy; resize = renderer.resize
  constructor(options: { video: HTMLVideoElement }) { renderer.options(options); const overlay = document.createElement('div'); overlay.className = 'JASSUB'; options.video.insertAdjacentElement('afterend', overlay) }
} }))
it('keeps subtitles off until selected and preserves ASS script and font bytes for libass', async () => {
  vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true)
  const script = '[Script Info]\n[Events]\nDialogue: {\\pos(20,30)\\k100}Text'
  vi.stubGlobal('fetch', vi.fn(async (url: string) => url.includes('/subtitles?')
    ? { ok: true, json: async () => ({ tracks: [{ path: 'embedded:2', label: 'Styled', format: 'ass' }], fonts: [3], truncated: false, warning: null }) }
    : url.includes('subtitle-font') ? { ok: true, arrayBuffer: async () => new Uint8Array([1, 2, 3]).buffer }
    : { ok: true, text: async () => script }))
  const host = document.createElement('div'), root = createRoot(host), video = document.createElement('video')
  const stage = document.createElement('div'); stage.append(video)
  renderer.options.mockClear(); renderer.destroy.mockClear()
  try {
    await act(async () => root.render(<VideoSubtitles video={video} sourceId="source" path="clip.mkv"/>))
    expect(renderer.options).not.toHaveBeenCalled()
    const select = host.querySelector('select')!
    await act(async () => { select.value = 'embedded:2'; select.dispatchEvent(new Event('change', { bubbles: true })) })
    expect(renderer.options).toHaveBeenCalledWith(expect.objectContaining({ video, subContent: script, fonts: [new Uint8Array([1, 2, 3])], queryFonts: false }))
    expect(stage.querySelector('.JASSUB')).not.toBeNull()
    video.remove() // React has already replaced the element during retry.
    await act(async () => { select.value = ''; select.dispatchEvent(new Event('change', { bubbles: true })) })
    expect(renderer.destroy).toHaveBeenCalledTimes(1)
    expect(stage.querySelector('.JASSUB')).toBeNull()
  } finally { await act(async () => root.unmount()); vi.unstubAllGlobals() }
})
it('preserves the selected track at original timing when retry replaces the video element', async () => {
  vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true)
  vi.stubGlobal('fetch', vi.fn(async (url: string) => url.includes('/subtitles?')
    ? { ok: true, json: async () => ({ tracks: [{ path: 'embedded:2', label: 'Styled', format: 'ass' }], fonts: [], truncated: false, warning: null }) }
    : { ok: true, text: async () => '[Script Info]\n[Events]' }))
  const host = document.createElement('div'), root = createRoot(host)
  document.body.append(host)
  const first = document.createElement('video'), second = document.createElement('video')
  renderer.options.mockClear()
  try {
    await act(async () => root.render(<VideoSubtitles video={first} sourceId="source" path="clip.mkv"/>))
    const select = host.querySelector('select')!
    await act(async () => { select.value = 'embedded:2'; select.dispatchEvent(new Event('change', { bubbles: true })) })
    expect(host.querySelector('input')).toBeNull()
    await act(async () => root.render(<VideoSubtitles video={second} sourceId="source" path="clip.mkv"/>))
    expect(renderer.options).toHaveBeenLastCalledWith(expect.objectContaining({ video: second, timeOffset: 0 }))
  } finally { await act(async () => root.unmount()); host.remove(); vi.unstubAllGlobals() }
})
it('shows the specific server failure and has no manual timing override',async()=>{
 vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT',true)
 vi.stubGlobal('fetch',vi.fn(async(url:string)=>url.includes('/subtitles?')
 ?{ok:true,json:async()=>({tracks:[{path:'embedded:2',label:'Styled',format:'ass'}],fonts:[],truncated:false,warning:null})}
 :{ok:false,json:async()=>({error:'Extracted media exceeds its 16 MiB output limit.'})}))
 const host=document.createElement('div'),root=createRoot(host),video=document.createElement('video')
 try{
 await act(async()=>root.render(<VideoSubtitles video={video} sourceId="s" path="film.mkv"/>))
 const select=host.querySelector('select')!
 await act(async()=>{select.value='embedded:2';select.dispatchEvent(new Event('change',{bubbles:true}))})
 expect(host.textContent).toContain('16 MiB output limit')
 expect(host.querySelector('input')).toBeNull()
 }finally{await act(async()=>root.unmount());vi.unstubAllGlobals()}
})
