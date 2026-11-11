// @vitest-environment jsdom
import {act} from 'react'
import {createRoot} from 'react-dom/client'
import {expect,it,vi} from 'vitest'
import MetadataEditor from './MetadataEditor'
import type {MediaItem,MetadataView} from './media'

it('reviews a Source Agent episode in the selected TheTVDB order',async()=>{
  vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT',true)
  Object.defineProperty(HTMLDialogElement.prototype,'showModal',{configurable:true,value:function(this:HTMLDialogElement){this.setAttribute('open','')}})
  Object.defineProperty(HTMLDialogElement.prototype,'close',{configurable:true,value:function(this:HTMLDialogElement){this.removeAttribute('open')}})
  const item:MediaItem={file_key:'fixture',source_id:'remote:source',source_name:'Agent',name:'Blue Box S01E01.mkv',
    remote:true,kind:'video',extension:'mkv',relative_path:'representation',available:true,modified_at:null,size_bytes:20,saved_at:null,last_opened_at:null}
  const fields={collection:'Blue Box',season:1,episode:1}
  const metadata:MetadataView={fields,has_custom:false,origin:'agent',artwork_url:null,can_edit:true,artwork_preference:'auto',provider:null,provider_id:null}
  const endpoint='/api/catalog/v1/remote-metadata/representation'
  const fetch=vi.fn(async(url:string)=>new Response(JSON.stringify(
    url.endsWith('/search')?[{id:'429934',title:'Blue Box'}]:
    url.endsWith('/preview')?{review_token:'review',provider:'tvdb',provider_id:'429934:10152847:absolute',fields:{...fields,title:'Chinatsu Senpai'},has_artwork:false}:
    url==='/api/metadata/providers'?[{provider:'tvdb',enabled:true}]:metadata
  )))
  vi.stubGlobal('fetch',fetch)
  const host=document.createElement('div'),root=createRoot(host)
  document.body.append(host)
  try{
    await act(async()=>root.render(<MetadataEditor item={item} onClose={vi.fn()} onUpdated={vi.fn()}/>))
    const search=[...host.querySelectorAll('button')].find(button=>button.textContent?.includes('Search provider'))!
    await act(async()=>search.click())
    const order=[...host.querySelectorAll('label')].find(label=>label.textContent?.startsWith('Episode order'))!.querySelector('select')!
    await act(async()=>{order.value='absolute';order.dispatchEvent(new Event('change',{bubbles:true}))})
    await act(async()=>[...host.querySelectorAll('button')].find(button=>button.textContent==='Preview match')!.click())
    expect(fetch).toHaveBeenCalledWith(endpoint+'/preview',expect.objectContaining({
      body:JSON.stringify({path:'representation',provider:'tvdb',id:'429934',order:'absolute',season:1,episode:1}),
    }))
    expect(host.textContent).toContain('Chinatsu Senpai')
    expect(host.querySelector('a[href="https://thetvdb.com/"]')).not.toBeNull()
  }finally{
    await act(async()=>root.unmount());host.remove();vi.restoreAllMocks();vi.unstubAllGlobals()
  }
})
