// @vitest-environment jsdom
import {act} from 'react'
import {createRoot} from 'react-dom/client'
import {expect,it,vi} from 'vitest'
import TranscodeOptions from './TranscodeOptions'
it('starts highest-quality transcoding without a viewer encoder override or direct playback option',async()=>{
 vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT',true)
 const fetch=vi.fn(async(url:string,_options?:RequestInit)=>new Response(JSON.stringify(url.includes('qualities')?[{height:480,bitrate_kbps:1000},{height:1080,bitrate_kbps:5000}]:{id:'session',encoder:'cpu',height:1080,bitrate_kbps:5000})))
 vi.stubGlobal('fetch',fetch)
 const host=document.createElement('div'),root=createRoot(host),onSelect=vi.fn()
 try{
 await act(async()=>root.render(<TranscodeOptions sourceId="s" path="film.mkv" profile={{height:0,audio:2}} onProfile={()=>{}} onSelect={onSelect}/>))
 expect(host.querySelector('[aria-label="Transcoding encoder"]')).toBeNull()
 expect(host.textContent).toContain('1080p · 5.0 Mbps')
 expect(host.textContent).not.toContain('direct playback')
 const post=fetch.mock.calls.find(([url])=>url==='/api/transcode')!
 expect(JSON.parse(post[1]!.body as string)).toEqual({source_id:'s',path:'film.mkv',height:0,audio:2})
 expect(onSelect).toHaveBeenCalledWith('/api/transcode/session/manifest.m3u8')
 }finally{await act(async()=>root.unmount());vi.unstubAllGlobals()}
})
