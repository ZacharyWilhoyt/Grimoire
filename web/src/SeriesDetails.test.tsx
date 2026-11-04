// @vitest-environment jsdom
import {act} from 'react'
import {createRoot} from 'react-dom/client'
import {afterEach,expect,it,vi} from 'vitest'
import SeriesDetails from './SeriesDetails'
import {apiFetch} from './api'
import type {MediaItem} from './media'
vi.mock('./api',()=>({apiFetch:vi.fn()}))
const mocked=vi.mocked(apiFetch)
const item=(n:number,kind:'book'|'video'='book')=>({file_key:String(n),kind,available:true,source_id:'s',source_name:'Source',relative_path:`file${n}`,name:`Title ${n}`,extension:kind==='book'?'cbz':'mkv',saved_at:null,last_opened_at:null,modified_at:n,size_bytes:1,progress:n===1?{position:3,total:20,updated_at:1}:null,metadata:{has_custom:false,origin:'local',artwork_url:null,can_edit:true,artwork_preference:'auto',provider:null,provider_id:null,fields:{title:`Title ${n}`,collection:'Series',volume:n,episode:n,genres:['Adventure'],description:'A paragraph.\n\n'+('A long synopsis. '.repeat(60))},credits:[],group:{id:'g',label:'Series',can_edit:false,has_custom_artwork:false,has_custom_genres:false}}} as MediaItem)
afterEach(()=>{vi.resetAllMocks();vi.unstubAllGlobals()})
async function fixture(kind:'book'|'video'='book'){
 vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT',true)
 mocked.mockImplementation(async(_url,init)=>new Response(JSON.stringify(init?.method==='PUT'?{...JSON.parse(String(init.body)),revision:2}:{status:'active',favorite:false,revision:1})))
 const host=document.createElement('div'),root=createRoot(host),open=vi.fn(),save=vi.fn()
 await act(async()=>root.render(<SeriesDetails series members={[item(2,kind),item(1,kind)]} catalog={[]} busy={false} onClose={vi.fn()} onOpen={open} onSave={save} onDetails={vi.fn()} onSeries={vi.fn()} onMetadata={vi.fn()} onGroupMetadata={vi.fn()}/>))
 return {host,root,open,save}
}
it('opens the reader with saved progress and preserves sorting and tab state during local interactions',async()=>{
 const {host,root,open,save}=await fixture()
 try{
  expect(host.querySelector('h1')?.textContent).toBe('Series')
  expect(host.querySelectorAll('tbody tr')).toHaveLength(2)
  expect(host.querySelector('tbody tr')?.textContent).toContain('Title 1')
  await act(async()=>host.querySelector<HTMLButtonElement>('.chapter-open')!.click())
  expect(open.mock.calls[0][0].progress.position).toBe(3)
  await act(async()=>host.querySelector<HTMLButtonElement>('[aria-label="Save Title 1"]')!.click())
  expect(save).toHaveBeenCalledOnce()
  await act(async()=>host.querySelector<HTMLButtonElement>('[aria-label="Sort descending"]')!.click())
  expect(host.querySelector('tbody tr')?.textContent).toContain('Title 2')
  await act(async()=>host.querySelector<HTMLButtonElement>('#series-tab-relations')!.click())
  expect(host.querySelector('#series-panel-content')?.hasAttribute('hidden')).toBe(true)
  expect(host.querySelector('#series-panel-relations')?.textContent).toContain('No relationship data')
  await act(async()=>host.querySelector<HTMLButtonElement>('#series-tab-content')!.click())
  expect(host.querySelector('tbody tr')?.textContent).toContain('Title 2')
  const readMore=[...host.querySelectorAll('button')].find(b=>b.textContent==='Read More')!
  await act(async()=>readMore.click())
  expect(host.querySelector('.series-synopsis p')?.textContent?.length).toBeGreaterThan(600)
  expect(host.textContent).not.toContain('Community rating')
 }finally{await act(async()=>root.unmount())}
})
it('persists status and favorites with revisions without starting playback, and shares a permission-scoped link',async()=>{
 const {host,root,open}=await fixture('video')
 try{
  const status=host.querySelector<HTMLSelectElement>('[aria-label="Your series status"]')!
  expect(status.value).toBe('active')
  await act(async()=>{status.value='on_hold';status.dispatchEvent(new Event('change',{bubbles:true}))})
  expect(JSON.parse(String(mocked.mock.calls.at(-1)?.[1]?.body))).toMatchObject({status:'on_hold',favorite:false,expected_revision:1})
  await act(async()=>host.querySelector<HTMLButtonElement>('[aria-label="Favorite series"]')!.click())
  expect(JSON.parse(String(mocked.mock.calls.at(-1)?.[1]?.body))).toMatchObject({status:'on_hold',favorite:true,expected_revision:2})
  expect(open).not.toHaveBeenCalled()
  await act(async()=>host.querySelector<HTMLButtonElement>('[aria-label="Share series link"]')!.click())
  expect(host.querySelector<HTMLInputElement>('[aria-label="Series share link"]')?.value).toContain('series=g')
  expect(host.textContent).toContain('Viewers still need access')
  expect(host.querySelector('#series-tab-content')?.textContent).toBe('Episodes')
  expect(host.querySelectorAll('.catalog-card')).toHaveLength(2)
 }finally{await act(async()=>root.unmount())}
})
it('shows preference failures without optimistic changes and filters the chapter list honestly',async()=>{
 const {host,root}=await fixture()
 try{
  mocked.mockResolvedValue(new Response(JSON.stringify({error:'Changed in another window'}),{status:409}))
  await act(async()=>host.querySelector<HTMLButtonElement>('[aria-label="Favorite series"]')!.click())
  expect(host.querySelector('[role="alert"]')?.textContent).toContain('Changed in another window')
  expect(host.querySelector('[aria-label="Favorite series"]')?.getAttribute('aria-pressed')).toBe('false')
  const input=host.querySelector<HTMLInputElement>('[aria-label="Filter series content"]')!
  await act(async()=>{Object.getOwnPropertyDescriptor(HTMLInputElement.prototype,'value')!.set!.call(input,'missing');input.dispatchEvent(new Event('input',{bubbles:true}))})
  expect(host.querySelectorAll('tbody tr')).toHaveLength(0)
  expect(host.textContent).toContain('No titles match')
 }finally{await act(async()=>root.unmount())}
})
