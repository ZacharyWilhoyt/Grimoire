// @vitest-environment jsdom
import {act} from 'react'
import {createRoot} from 'react-dom/client'
import {expect,it,vi} from 'vitest'
import MediaCollection from './MediaCollection'
import type {MediaItem} from './media'
function item(key:string,collection:string|null,edit=true):MediaItem{return {file_key:key,source_id:'s',source_name:'Source',name:key,relative_path:key+'.mp3',kind:'music',available:true,extension:'mp3',size_bytes:100,modified_at:null,saved_at:null,last_opened_at:null,metadata:{fields:{title:key,creator:'Artist',collection},can_edit:edit,group:collection?{id:'album',label:'Artist — Album',can_edit:edit,has_custom_genres:false,has_custom_artwork:false}:null,origin:'local',has_custom:false,artwork_url:null,artwork_preference:'auto',provider:null,provider_id:null}}}
it('drops an editable entry onto a group and refreshes membership without touching individual identity',async()=>{
 vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT',true)
 const loose=item('Loose',null),target=item('Song','Album')
 const result={...loose.metadata!,fields:{...loose.metadata!.fields,collection:'Album'},group:target.metadata!.group}
 const fetch=vi.fn(async()=>new Response(JSON.stringify(result)));vi.stubGlobal('fetch',fetch)
 const host=document.createElement('div'),root=createRoot(host)
 try{
 await act(async()=>root.render(<MediaCollection items={[loose,target]} busy={false} onOpen={()=>{}} onSave={()=>{}} onMetadata={()=>{}} onGroupMetadata={()=>{}}/>))
 const entry=host.querySelector<HTMLElement>('article[draggable=true]')!,folder=host.querySelector<HTMLElement>('.media-folder')!
 const data=new Map<string,string>(),transfer={setData:(k:string,v:string)=>data.set(k,v),getData:(k:string)=>data.get(k)??'',effectAllowed:'',dropEffect:''}
 const event=(name:string)=>{const e=new Event(name,{bubbles:true,cancelable:true});Object.defineProperty(e,'dataTransfer',{value:transfer});return e}
 await act(async()=>entry.dispatchEvent(event('dragstart')))
 const over=event('dragover');await act(async()=>folder.dispatchEvent(over));expect(over.defaultPrevented).toBe(true)
 await act(async()=>folder.dispatchEvent(event('drop')))
 expect(JSON.parse((fetch.mock.calls as unknown as [string,RequestInit][])[0][1].body as string)).toEqual({path:'Loose.mp3',target_path:'Song.mp3',expected_group_id:'album'})
 expect(host.textContent).toContain('Loose is now in Artist — Album')
 expect(host.querySelector('article[draggable=true]')).toBeNull()
 }finally{await act(async()=>root.unmount());vi.unstubAllGlobals()}
})
it('keeps read-only media outside grouping controls',async()=>{
 vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT',true)
 const host=document.createElement('div'),root=createRoot(host)
 try{await act(async()=>root.render(<MediaCollection items={[item('Loose',null,false),item('Song','Album',false)]} busy={false} onOpen={()=>{}} onSave={()=>{}} onMetadata={()=>{}} onGroupMetadata={()=>{}}/>));expect(host.querySelector('[draggable=true]')).toBeNull();expect(host.textContent).not.toContain('Move to group')}
 finally{await act(async()=>root.unmount());vi.unstubAllGlobals()}
})
it('opens book details from cover clicks without starting the reader',async()=>{
 vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT',true)
 const book={...item('Novel',null,false),kind:'book' as const,extension:'epub',relative_path:'novel.epub'}
 const host=document.createElement('div'),root=createRoot(host),details=vi.fn(),open=vi.fn()
 try{
  await act(async()=>root.render(<MediaCollection items={[book]} busy={false} onOpen={open} onDetails={details} onSave={()=>{}} onMetadata={()=>{}} onGroupMetadata={()=>{}}/>))
  await act(async()=>host.querySelector<HTMLButtonElement>('[aria-label="Details Novel"]')!.click())
  expect(details).toHaveBeenCalledExactlyOnceWith(book);expect(open).not.toHaveBeenCalled()
 }finally{await act(async()=>root.unmount());vi.unstubAllGlobals()}
})
