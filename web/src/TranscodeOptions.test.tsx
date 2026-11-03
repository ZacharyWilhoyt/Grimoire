// @vitest-environment jsdom
import {act} from 'react'
import {createRoot} from 'react-dom/client'
import {expect,it,vi} from 'vitest'
import TranscodeOptions from './TranscodeOptions'
it('disables unavailable GPU encoders and preserves audio selection when applying a video profile',async()=>{
 vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT',true)
 const fetch=vi.fn().mockResolvedValueOnce(new Response(JSON.stringify([{encoder:'cpu',available:true},{encoder:'nvidia',available:false}]))).mockResolvedValueOnce(new Response(JSON.stringify({id:'session',encoder:'cpu'})))
 vi.stubGlobal('fetch',fetch)
 const host=document.createElement('div'),root=createRoot(host),onSelect=vi.fn()
 try{
 await act(async()=>root.render(<TranscodeOptions sourceId="s" path="film.mkv" profile={{encoder:'cpu',height:720,audio:2}} onProfile={()=>{}} onSelect={onSelect}/>))
 expect(host.querySelector<HTMLOptionElement>('option[value="nvidia"]')!.disabled).toBe(true)
 await act(async()=>[...host.querySelectorAll('button')].find(b=>b.textContent==='Apply transcoding')!.click())
 expect(JSON.parse(fetch.mock.calls[1][1].body)).toMatchObject({encoder:'cpu',height:720,audio:2})
 expect(onSelect).toHaveBeenCalledWith('/api/transcode/session/manifest.m3u8')
 await act(async()=>[...host.querySelectorAll('button')].find(b=>b.textContent==='Original / direct playback')!.click())
 expect(onSelect).toHaveBeenLastCalledWith(null)
 }finally{await act(async()=>root.unmount());vi.unstubAllGlobals()}
})
