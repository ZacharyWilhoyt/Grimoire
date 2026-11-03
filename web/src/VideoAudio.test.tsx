// @vitest-environment jsdom
import { act } from 'react'
import { createRoot } from 'react-dom/client'
import { expect, it, vi } from 'vitest'
import VideoAudio from './VideoAudio'
it('prepares the selected stream, pauses, and retains the previous selection on failure', async () => {
  vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true)
  const fetch = vi.fn().mockResolvedValueOnce(new Response(JSON.stringify([{index:2,label:'Japanese',codec:'ac3',default:false}]))).mockResolvedValueOnce(new Response(JSON.stringify({id:'prepared'}))).mockResolvedValueOnce(new Response(JSON.stringify({error:'Processing busy'}),{status:429}))
  vi.stubGlobal('fetch',fetch)
  const host = document.createElement('div'), root = createRoot(host), onSelect = vi.fn(), video = document.createElement('video')
  const pause = vi.spyOn(video,'pause').mockImplementation(() => {})
  try {
    await act(async () => root.render(<VideoAudio sourceId="source" path="episode.mkv" video={video} onSelect={onSelect}/>))
    const select = host.querySelector('select')!
    await act(async () => { select.value='2'; select.dispatchEvent(new Event('change',{bubbles:true})) })
    expect(pause).toHaveBeenCalledOnce()
    expect(JSON.parse(fetch.mock.calls[1][1].body)).toEqual({stream_index:2})
    expect(onSelect).toHaveBeenCalledWith('/api/sources/source/video-audio/prepared?path=episode.mkv')
    await act(async () => host.querySelector('button')!.click())
    expect(host.textContent).toContain('Processing busy')
    expect(onSelect).toHaveBeenCalledOnce()
    expect(select.value).toBe('2')
    await act(async () => { select.value=''; select.dispatchEvent(new Event('change',{bubbles:true})) })
    expect(onSelect).toHaveBeenLastCalledWith(null)
  } finally { await act(async () => root.unmount()); vi.restoreAllMocks(); vi.unstubAllGlobals() }
})
