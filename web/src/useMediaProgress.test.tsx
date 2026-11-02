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
