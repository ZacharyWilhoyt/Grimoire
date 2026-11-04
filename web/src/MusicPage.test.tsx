// @vitest-environment jsdom
import {act} from 'react'
import {createRoot} from 'react-dom/client'
import {expect,it,vi} from 'vitest'
import MusicPage from './MusicPage'
import {apiFetch} from './api'
import type {MediaItem} from './media'
vi.mock('./api',()=>({apiFetch:vi.fn()}))
const items=[['a','Ada','First'],['b','Ada','Second'],['c','Bea','Third']].map(([key,creator,collection],index)=>({file_key:key.repeat(64),source_id:'source',source_name:'Source',name:key,relative_path:key+'.mp3',kind:'music',extension:'mp3',available:true,saved_at:null,metadata:{fields:{title:`Song ${key}`,creator,collection,genres:['jazz'],track:index+1,duration:180},artwork_url:null}} as MediaItem))
it('filters artists/albums without playing, preserves filters across tabs, and saves favorite hearts independently',async()=>{
 vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT',true)
 vi.mocked(apiFetch).mockImplementation(async(_url,options)=>new Response(JSON.stringify(options?.method==='PUT'?{revision:2,library:JSON.parse(options.body as string).library}:{revision:1,library:{favorites:[],playlists:[]}})))
 const host=document.createElement('div'),root=createRoot(host),open=vi.fn()
 const button=(text:string)=>[...host.querySelectorAll('button')].find(b=>b.textContent===text)!
 try{
  await act(async()=>root.render(<MusicPage active items={items} query="" loading={false} error="" note="" busy={false} onOpen={open} onSave={()=>{}} onMetadata={()=>{}} onRefresh={()=>{}}/>))
  expect(open).not.toHaveBeenCalled()
  await act(async()=>button('Adajazz').click())
  expect(host.querySelector('tbody')?.textContent).not.toContain('Song c')
  expect(host.querySelectorAll('.music-browser')[1].textContent).not.toContain('Third')
  await act(async()=>host.querySelector<HTMLButtonElement>('[aria-label="Favorite artist Ada"]')!.click())
  expect(host.querySelector<HTMLButtonElement>('[aria-label="Favorite artist Ada"]')!.getAttribute('aria-pressed')).toBe('true')
  expect(button('Adajazz').getAttribute('aria-pressed')).toBe('true')
  await act(async()=>button('Playlists').click())
  await act(async()=>button('Songs').click())
  expect(button('Adajazz').getAttribute('aria-pressed')).toBe('true')
  await act(async()=>host.querySelector<HTMLButtonElement>('.music-title-play')!.click())
  expect(open.mock.calls[0][1]).toHaveLength(2)
 }finally{await act(async()=>root.unmount());vi.unstubAllGlobals()}
})
