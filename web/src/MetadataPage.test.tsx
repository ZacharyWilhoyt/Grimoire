// @vitest-environment jsdom
import {act} from 'react'
import {createRoot} from 'react-dom/client'
import {expect,it,vi} from 'vitest'
import MetadataPage from './MetadataPage'
import type {MediaItem} from './media'

it('links an owned remote file to an accessible collection without copying item metadata',async()=>{
 vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT',true)
 const own:MediaItem={file_key:'own',location_id:'origin',source_id:'remote:mine',source_name:'Mine',remote:true,
   name:'New episode',kind:'video',extension:'mkv',relative_path:'opaque',available:true,size_bytes:10,modified_at:null,saved_at:null,last_opened_at:null}
 const target:MediaItem={...own,file_key:'target',location_id:'anchor',source_id:'other',source_name:'Other',name:'Existing episode',
   metadata:{fields:{collection:'Existing series'},group:{id:'group',label:'Existing series',can_edit:false,has_custom_artwork:false,has_custom_genres:false},
   has_custom:false,can_edit:false,origin:'local',artwork_url:null,artwork_preference:'auto',provider:null,provider_id:null}}
 own.metadata={...target.metadata!,can_edit:true,group:null,fields:{}}
 const fetch=vi.fn(async()=>new Response(null,{status:204}))
 vi.stubGlobal('fetch',fetch)
 const refresh=vi.fn(),host=document.createElement('div'),root=createRoot(host)
 try {
  await act(async()=>root.render(<MetadataPage sources={[{id:'remote:mine',name:'Mine',remote:true}]}
    items={[own,target]} query="" loading={false} error="" onRefresh={refresh} onMetadata={vi.fn()} onGroup={vi.fn()}/>))
  expect([...host.querySelectorAll('button')].some(button=>button.textContent==='Edit metadata')).toBe(true)
  expect(host.querySelector('[aria-label="Select Existing episode"]')).toBeNull()
  await act(async()=>host.querySelector<HTMLInputElement>('[aria-label="Select New episode"]')!.click())
  const select=[...host.querySelectorAll('select')][1]
  await act(async()=>{select.value='anchor';select.dispatchEvent(new Event('change',{bubbles:true}))})
  await act(async()=>[...host.querySelectorAll('button')].find(button=>button.textContent==='Add to collection')!.click())
  expect(fetch).toHaveBeenCalledWith('/api/catalog/v1/collection-membership',expect.objectContaining({
    method:'POST',body:JSON.stringify({locations:['origin'],target:'anchor',group:'group'}),
  }))
  expect(refresh).toHaveBeenCalledOnce()
 } finally {await act(async()=>root.unmount());vi.unstubAllGlobals()}
})
