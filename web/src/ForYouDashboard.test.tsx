// @vitest-environment jsdom
import {act} from 'react'
import {createRoot} from 'react-dom/client'
import {expect,it,vi} from 'vitest'
import ForYouDashboard from './ForYouDashboard'
import type {MediaItem} from './media'
vi.mock('./api',()=>({apiFetch:vi.fn(async()=>new Response(JSON.stringify({watch_seconds:null,episodes_watched:0,reading_units:0,reading_active:1,books_completed:0,series_completed:null,watching_genres:[],reading_genres:[],truncated:false})))}))
it('scrolls each continuation rail independently and resumes original user progress directly',async()=>{
 vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT',true)
 const video={file_key:'video',kind:'video',available:true,name:'Episode',extension:'mp4',progress:{position:42,total:100,updated_at:1},metadata:{fields:{episode:3,collection:'Show'}}} as MediaItem
 const book={file_key:'book',kind:'book',available:true,name:'Book',extension:'epub',progress:{position:3,total:20,updated_at:1}} as MediaItem
 const host=document.createElement('div'),root=createRoot(host),open=vi.fn()
 try{
  await act(async()=>root.render(<ForYouDashboard items={[video,book]} catalog={[]} picks={null} onOpen={open} busy={false}/>))
  expect(host.querySelector('.resume-strip')).toBeNull()
  const rails=[...host.querySelectorAll<HTMLElement>('.continuation-rail')]
  for(const rail of rails){Object.defineProperty(rail,'clientWidth',{value:300});Object.defineProperty(rail,'scrollWidth',{value:900});rail.scrollBy=vi.fn()}
  await act(async()=>window.dispatchEvent(new Event('resize')))
  await act(async()=>host.querySelector<HTMLButtonElement>('[aria-label="Scroll Continue Watching right"]')!.click())
  expect(rails[0].scrollBy).toHaveBeenCalledOnce();expect(rails[1].scrollBy).not.toHaveBeenCalled()
  await act(async()=>host.querySelector<HTMLButtonElement>('[aria-label="Resume Episode"]')!.click())
  expect(open).toHaveBeenLastCalledWith(video,[video]);expect(open.mock.calls[0][0].progress.position).toBe(42)
  await act(async()=>host.querySelector<HTMLButtonElement>('[aria-label="Resume reading Book"]')!.click())
  expect(open).toHaveBeenLastCalledWith(book,[book]);expect(host.textContent).toContain('Chapter 4')
  expect(host.textContent).toContain('Preferred Watching Genres');expect(host.textContent).toContain('Preferred Reading Genres')
 }finally{await act(async()=>root.unmount());vi.unstubAllGlobals()}
})
