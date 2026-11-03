// @vitest-environment jsdom
import {act} from 'react'
import {createRoot} from 'react-dom/client'
import {expect,it,vi} from 'vitest'
import SourceTranscoding from './SourceTranscoding'
it('only probes encoders when the owner opens settings and saves source policy',async()=>{
 vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT',true)
 const fetch=vi.fn(async(url:string,options?:RequestInit)=>options?.method==='PUT'?new Response(null,{status:204}):new Response(JSON.stringify(url.includes('capabilities')?[{encoder:'cpu',available:true},{encoder:'amd',available:true}]:{encoder:'cpu'})))
 vi.stubGlobal('fetch',fetch)
 const host=document.createElement('div'),root=createRoot(host)
 try{
 await act(async()=>root.render(<SourceTranscoding sourceId="source"/>));expect(fetch).not.toHaveBeenCalled()
 await act(async()=>{host.querySelector('details')!.open=true;host.querySelector('details')!.dispatchEvent(new Event('toggle'))})
 expect(host.querySelector('select')!.value).toBe('cpu')
 await act(async()=>{const s=host.querySelector('select')!;s.value='amd';s.dispatchEvent(new Event('change',{bubbles:true}))})
 await act(async()=>host.querySelector('button')!.click())
 const call=fetch.mock.calls.find(([,o])=>o?.method==='PUT')!
 expect(JSON.parse(call[1]!.body as string)).toEqual({encoder:'amd'})
 expect(host.textContent).toContain('Source encoder saved')
 }finally{await act(async()=>root.unmount());vi.unstubAllGlobals()}
})
