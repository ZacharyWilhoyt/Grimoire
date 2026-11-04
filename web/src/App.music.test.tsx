// @vitest-environment jsdom
import {act} from 'react'
import {createRoot} from 'react-dom/client'
import {expect,it,vi} from 'vitest'
import App from './App'
import {apiFetch} from './api'
import type {MediaItem} from './media'
vi.mock('./Authentication',()=>({default:({children}:{children:(user:unknown,logout:()=>void)=>unknown})=>children({id:'test',username:'Test',is_admin:false},()=>{})}))
vi.mock('./api',()=>({apiFetch:vi.fn()}))
const song={file_key:'a'.repeat(64),source_id:'s',source_name:'Source',relative_path:'song.mp3',name:'Test song',kind:'music',extension:'mp3',available:true,saved_at:null,last_opened_at:null,size_bytes:1,modified_at:0} as MediaItem
it('keeps one audio element through internal navigation and starts idle after fresh initialization',async()=>{
 vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT',true)
 window.history.replaceState(null,'','/#music')
 vi.mocked(apiFetch).mockImplementation(async(url,options)=>new Response(JSON.stringify(url==='/api/sources'?[{id:'s',name:'Source',can_access_media:true,available:true}]:url==='/api/catalog'?{items:[song],truncated:false,unavailable_sources:0}:url==='/api/music/library'?{revision:1,library:{favorites:['artist:test'],playlists:[]}}:url==='/api/activity/open'?{file_key:song.file_key,saved_at:null,last_opened_at:1}:url.startsWith('/api/progress')?(options?.method==='PUT'?{revision:1}:null):{items:[],has_more:false,next_offset:0,unavailable_items:0})))
 const host=document.createElement('div'),root=createRoot(host)
 document.body.append(host)
 const button=(name:string)=>[...host.querySelectorAll('button')].find(button=>button.textContent===name)!
 try{
  await act(async()=>root.render(<App/>))
  expect(host.querySelector('audio')).toBeNull()
  await act(async()=>button('Test song').click())
  const audio=host.querySelector('audio')!
  expect(audio).not.toBeNull()
  await act(async()=>button('Discover').click())
  expect(host.querySelector('audio')).toBe(audio)
  expect(host.querySelectorAll('audio')).toHaveLength(1)
  expect(host.querySelector('.catalog-grid')?.textContent??'').not.toContain('Test song')
  await act(async()=>button('Music').click())
  expect(host.querySelector('audio')).toBe(audio)
  await act(async()=>root.render(<App key="fresh"/>))
  expect(host.querySelector('audio')).toBeNull()
 }finally{await act(async()=>root.unmount());host.remove();vi.unstubAllGlobals();window.history.replaceState(null,'','/')}
})
