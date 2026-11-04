// @vitest-environment jsdom
import {act,type ComponentProps} from 'react'
import {createRoot} from 'react-dom/client'
import {expect,it,vi} from 'vitest'
import CatalogViews from './CatalogViews'
import type {MediaItem} from './media'
it('hides Discover card actions while preserving cover navigation and actions in My library',async()=>{
 vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT',true)
 const book:MediaItem={file_key:'book',kind:'book',available:true,source_id:'s',source_name:'Source',relative_path:'book.cbz',name:'Book',extension:'cbz',size_bytes:1,modified_at:null,saved_at:null,last_opened_at:null}
 const series:MediaItem={...book,file_key:'episode',kind:'video',name:'Episode',relative_path:'episode.mkv',extension:'mkv',metadata:{fields:{collection:'Series'},group:{id:'g',label:'Series',can_edit:false,has_custom_artwork:false,has_custom_genres:false},has_custom:false,can_edit:false,origin:'local',artwork_url:null,artwork_preference:'auto',provider:null,provider_id:null}}
 const details=vi.fn(),openSeries=vi.fn()
 const props:ComponentProps<typeof CatalogViews>={page:'Discover',username:'Test',catalog:[book,series],saved:[book,series],recent:[],continuing:[],loading:false,error:'',note:'',query:'',kind:'',source:'',sort:'title',sources:[],busy:false,hasMore:false,genre:'',groupBy:'collection',onDetails:details,onSeries:openSeries,onOpen:vi.fn(),onSave:vi.fn(),onMetadata:vi.fn(),onGroupMetadata:vi.fn(),onGenre:vi.fn(),onGroup:vi.fn(),onKind:vi.fn(),onSource:vi.fn(),onSort:vi.fn(),onRefresh:vi.fn(),onLoadMore:vi.fn(),onDiscover:vi.fn()}
 const host=document.createElement('div'),root=createRoot(host)
 try{
  await act(async()=>root.render(<CatalogViews {...props}/>))
  expect(host.querySelector('.media-actions')).toBeNull()
  await act(async()=>host.querySelector<HTMLButtonElement>('[aria-label="Details Book"]')!.click())
  expect(details).toHaveBeenCalledExactlyOnceWith(book)
  await act(async()=>host.querySelector<HTMLButtonElement>('[aria-label="Open folder: Series"]')!.click())
  expect(openSeries).toHaveBeenCalledExactlyOnceWith(series)
  await act(async()=>root.render(<CatalogViews {...props} page="My library"/>))
  expect(host.querySelectorAll('.media-actions')).toHaveLength(2)
 }finally{await act(async()=>root.unmount());vi.unstubAllGlobals()}
})
