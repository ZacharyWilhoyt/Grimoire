// @vitest-environment jsdom
import { act } from 'react'
import { createRoot } from 'react-dom/client'
import { afterEach, expect, it, vi } from 'vitest'
import { useMediaProgress } from './useMediaProgress'
vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true)
afterEach(() => vi.restoreAllMocks())
it('restores after metadata arrives and saves the old element when switching files', async () => {
  const fetch = vi.fn(async (_url: unknown, options?: RequestInit) => new Response(JSON.stringify(options?.method === 'PUT' ? { revision: 2 } : { position: 25, total: 100, locator: null, revision: 1 })))
  vi.stubGlobal('fetch',fetch)
  vi.spyOn(HTMLMediaElement.prototype,'play').mockResolvedValue()
  const host = document.createElement('div'); document.body.append(host); const root = createRoot(host)
  function Player({ path }: { path: string }) { const { media } = useMediaProgress<HTMLAudioElement>('source',path); return <audio key={path} ref={media}/> }
  await act(async () => { root.render(<Player path="first.mp3"/>) })
  const first=host.querySelector('audio')!
  Object.defineProperty(first,'readyState',{value:1}); Object.defineProperty(first,'duration',{value:100})
  await act(async () => { first.dispatchEvent(new Event('loadedmetadata')) })
  expect(first.currentTime).toBe(25)
  first.currentTime=40
  await act(async () => { root.render(<Player path="second.mp3"/>) })
  const saves=fetch.mock.calls.filter(([,options])=>options?.method==='PUT')
  expect(JSON.parse(saves[0][1]!.body as string)).toMatchObject({relative_path:'first.mp3',position:40,expected_revision:1})
  await act(async () => root.unmount());host.remove();vi.unstubAllGlobals()
})
it('flushes progress before an audio-resource switch and restores time and volume on the new element', async () => {
  vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true)
  let stored = 25, release!: () => void
  const fetch = vi.fn(async (_url: unknown, options?: RequestInit) => {
    if (options?.method === 'PUT') {
      stored = JSON.parse(options.body as string).position
      await new Promise<void>(resolve => { release = resolve })
      return new Response(JSON.stringify({ revision: 2 }))
    }
    return new Response(JSON.stringify({ position: stored, total: 100, locator: null, revision: stored === 25 ? 1 : 2 }))
  })
  vi.stubGlobal('fetch',fetch)
  vi.spyOn(HTMLMediaElement.prototype,'play').mockResolvedValue()
  const host = document.createElement('div'), root = createRoot(host)
  function Player({ resource }: { resource: string }) { const { media } = useMediaProgress<HTMLVideoElement>('source','original.mkv',resource); return <video key={resource} ref={media}/> }
  try {
    await act(async () => root.render(<Player resource="original"/>))
    const first = host.querySelector('video')!
    Object.defineProperty(first,'readyState',{value:1}); Object.defineProperty(first,'duration',{value:100})
    await act(async () => first.dispatchEvent(new Event('loadedmetadata')))
    expect(first.currentTime).toBe(25)
    first.currentTime=40; first.volume=.4
    await act(async () => root.render(<Player resource="selected-aac"/>))
    expect(fetch.mock.calls.filter(([,options]) => options?.method !== 'PUT')).toHaveLength(1)
    const second = host.querySelector('video')!
    Object.defineProperty(second,'readyState',{value:1}); Object.defineProperty(second,'duration',{configurable:true,value:100})
    await act(async () => { second.dispatchEvent(new Event('loadedmetadata')); release() })
    expect(second.currentTime).toBe(40)
    expect(second.volume).toBe(.4)
    expect(JSON.parse(fetch.mock.calls.find(([,options]) => options?.method === 'PUT')![1]!.body as string).relative_path).toBe('original.mkv')
    // Avoid another unresolved write during teardown.
    Object.defineProperty(second,'duration',{value:NaN})
  } finally { await act(async () => root.unmount()); vi.restoreAllMocks(); vi.unstubAllGlobals() }
})

it('does not restore an old music position when restoration is disabled',async()=>{
 vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT',true)
 vi.stubGlobal('fetch',vi.fn(async()=>new Response(JSON.stringify({position:73,total:100,locator:null,revision:1}))))
 const play=vi.spyOn(HTMLMediaElement.prototype,'play').mockResolvedValue()
 const host=document.createElement('div'),root=createRoot(host)
 function Player(){const {media}=useMediaProgress<HTMLAudioElement>('source','song.mp3','',false);return <audio ref={media}/>}
 try{
  await act(async()=>root.render(<Player/>))
  const audio=host.querySelector('audio')!
  Object.defineProperty(audio,'readyState',{value:1});Object.defineProperty(audio,'duration',{value:100})
  await act(async()=>audio.dispatchEvent(new Event('loadedmetadata')))
  expect(audio.currentTime).toBe(0)
  expect(play).toHaveBeenCalledOnce()
 }finally{await act(async()=>root.unmount());vi.unstubAllGlobals()}
})
