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

it('switches an interrupted track without replacing the rest of the queue',async()=>{
 vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT',true)
 vi.spyOn(HTMLMediaElement.prototype,'play').mockResolvedValue()
 window.history.replaceState(null,'','/#music')
 const remote:MediaItem={...song,file_key:'b'.repeat(64),logical_id:'logical-track',remote:true,source_id:'remote:test',source_name:'Remote',media_url:'/api/nodes/v1/sources/test/representations/song'}
 const local:MediaItem={...song,logical_id:remote.logical_id}
 const following:MediaItem={...song,file_key:'c'.repeat(64),name:'Z following song',relative_path:'following.mp3'}
 const locations=[{id:'first',source:'Remote',extension:'mp3',size_bytes:1},{id:'second',source:'Source',extension:'mp3',size_bytes:1}]
 const fetch=vi.mocked(apiFetch)
 fetch.mockImplementation(async(url,options)=>{
  const data=url==='/api/sources'?[{id:'s',name:'Source',can_access_media:true,available:true}]
    :url==='/api/catalog'?{items:[remote,following],truncated:false,unavailable_sources:0}
    :url==='/api/music/library'?{revision:1,library:{favorites:[],playlists:[]}}
    :url==='/api/playback/v1'?{session:'fixture',location:'first',item:remote,options:locations,exact:true}
    :url==='/api/playback/v1/fixture/switch'?{session:'fixture',location:'second',item:local,options:locations.slice(1),exact:true}
    :url==='/api/activity/open'?{file_key:remote.file_key,saved_at:null,last_opened_at:1}
    :url.startsWith('/api/progress')?(options?.method==='PUT'?{revision:1}:null)
    :{items:[],has_more:false,next_offset:0,unavailable_items:0}
  return new Response(JSON.stringify(data))
 })
 const host=document.createElement('div'),root=createRoot(host)
 const load=async(audio:HTMLAudioElement)=>{
  Object.defineProperties(audio,{readyState:{value:1},duration:{value:100}})
  await act(async()=>audio.dispatchEvent(new Event('loadedmetadata')))
 }
 try{
  await act(async()=>root.render(<App/>))
  const play=[...host.querySelectorAll('button')].find(button=>button.textContent==='Test song')!
  await act(async()=>play.click())
  const first=host.querySelector('audio')!
  expect(first.getAttribute('src')).toBe(remote.media_url)
  await load(first)
  first.currentTime=37
  await act(async()=>first.dispatchEvent(new Event('error')))
  const second=host.querySelector('audio')!
  expect(second.getAttribute('src')).toBe('/api/sources/s/audio?path=song.mp3')
  await load(second)
  expect(second.currentTime).toBe(37)
  expect(host.textContent).toContain('1 of 2')
  expect(fetch.mock.calls.filter(([url])=>url==='/api/playback/v1/fixture/switch')).toHaveLength(1)
 }finally{
  await act(async()=>root.unmount())
  vi.restoreAllMocks();vi.unstubAllGlobals();window.history.replaceState(null,'','/')
 }
})
