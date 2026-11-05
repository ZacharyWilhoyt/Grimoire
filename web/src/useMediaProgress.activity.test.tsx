// @vitest-environment jsdom
import {act} from 'react'
import {createRoot} from 'react-dom/client'
import {expect,it,vi} from 'vitest'
import {useMediaProgress} from './useMediaProgress'
it('records elapsed playback and excludes forward seeks from watch time',async()=>{
 vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT',true)
 let time=0,revision=0;const payloads:{played_seconds?:number}[]=[]
 vi.spyOn(performance,'now').mockImplementation(()=>time)
 vi.spyOn(HTMLMediaElement.prototype,'play').mockResolvedValue()
 vi.stubGlobal('fetch',vi.fn(async(_url,options)=>{if(options?.method==='PUT'){payloads.push(JSON.parse(options.body));return new Response(JSON.stringify({revision:++revision}))}return new Response('null')}))
 const host=document.createElement('div'),root=createRoot(host)
 function Player(){const {media}=useMediaProgress<HTMLVideoElement>('source','clip.mp4');return <video ref={media}/>}
 try{
  await act(async()=>root.render(<Player/>))
  const video=host.querySelector('video')!
  Object.defineProperty(video,'readyState',{value:1});Object.defineProperty(video,'duration',{value:100});Object.defineProperty(video,'paused',{configurable:true,value:false})
  await act(async()=>{video.dispatchEvent(new Event('loadedmetadata'));video.dispatchEvent(new Event('play'))})
  await act(async()=>{time=5000;video.currentTime=5;video.dispatchEvent(new Event('timeupdate'))})
  await act(async()=>{time=6000;video.currentTime=90;video.dispatchEvent(new Event('seeking'));video.dispatchEvent(new Event('seeked'))})
  await act(async()=>{time=11000;video.currentTime=95;Object.defineProperty(video,'paused',{value:true});video.dispatchEvent(new Event('pause'))})
  expect(payloads.reduce((sum,p)=>sum+(p.played_seconds??0),0)).toBeCloseTo(10)
 }finally{await act(async()=>root.unmount());vi.restoreAllMocks();vi.unstubAllGlobals()}
})
