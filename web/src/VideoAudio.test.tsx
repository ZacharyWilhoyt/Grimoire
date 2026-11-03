// @vitest-environment jsdom
import { act } from 'react'
import { createRoot } from 'react-dom/client'
import { expect, it, vi } from 'vitest'
import VideoAudio from './VideoAudio'
it('prepares the selected stream, pauses, and retains the previous selection on failure', async () => {
  vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true)
  const fetch = vi.fn().mockResolvedValueOnce(new Response(JSON.stringify([{index:2,label:'Japanese',codec:'ac3',default:false}]))).mockResolvedValueOnce(new Response(JSON.stringify({id:'prepared'}))).mockResolvedValueOnce(new Response(JSON.stringify({error:'Processing busy'}),{status:429})).mockResolvedValueOnce(new Response(JSON.stringify({id:'default'})))
  vi.stubGlobal('fetch',fetch)
  const host = document.createElement('div'), root = createRoot(host), onSelect = vi.fn(), video = document.createElement('video')
  const pause = vi.spyOn(video,'pause').mockImplementation(() => {})
  try {
    await act(async () => root.render(<VideoAudio sourceId="source" path="episode.mkv" video={video} onSelect={onSelect}/>))
    const select = [...host.querySelectorAll('button')].find(b=>b.textContent?.includes('Japanese'))!
    await act(async () => { select.click() })
    expect(pause).toHaveBeenCalledOnce()
    expect(JSON.parse(fetch.mock.calls[1][1].body)).toMatchObject({audio:2,height:0})
    expect(onSelect).toHaveBeenCalledWith('/api/transcode/prepared/manifest.m3u8')
    await act(async () => select.click())
    expect(host.textContent).toContain('Processing busy')
    expect(onSelect).toHaveBeenCalledOnce()
    expect(select.getAttribute('aria-pressed')).toBe('true')
    await act(async () => { [...host.querySelectorAll('button')].find(b=>b.textContent==='Source default audio')!.click() })
    expect(onSelect).toHaveBeenCalledTimes(2)
  } finally { await act(async () => root.unmount()); vi.restoreAllMocks(); vi.unstubAllGlobals() }
})
