import {useEffect,useRef,useState} from 'react'
import {archivePath,isImage,parseXml,type BookArchive} from './bookArchive'
import {sanitizeChapter,chapterDocument} from './epubContent'
import type {ReaderSettings} from './readerSettings'
export default function ReaderPage({archive,path,index,comic,settings,onReady,onVisible,onCozy,lazy=false}:{archive:BookArchive;path:string;index:number;comic:boolean;settings:ReaderSettings;onReady:(index:number,width:number,height:number)=>void;onVisible?:(index:number)=>void;onCozy:()=>void;lazy?:boolean}){
 const root=useRef<HTMLDivElement>(null),iframe=useRef<HTMLIFrameElement>(null)
 const [near,setNear]=useState(!lazy),[src,setSrc]=useState(''),[html,setHtml]=useState(''),[error,setError]=useState('')
 const disposeFrame=useRef<(()=>void)|undefined>(undefined)
 useEffect(()=>()=>disposeFrame.current?.(),[])
 const [reserved,setReserved]=useState<number|undefined>(undefined)
 const [height,setHeight]=useState<number|undefined>(undefined)
 useEffect(()=>{
   if(!lazy||!root.current||typeof IntersectionObserver==='undefined'){setNear(true);return}
   const observer=new IntersectionObserver(entries=>{for(const entry of entries){if(!entry.isIntersecting&&root.current)setReserved(root.current.getBoundingClientRect().height/settings.zoom*100);setNear(entry.isIntersecting)}},{root:root.current.closest('.reader-viewport'),rootMargin:'100% 0px'})
   observer.observe(root.current);return()=>observer.disconnect()
 },[lazy])
 useEffect(()=>{
   if(!root.current||!onVisible||typeof IntersectionObserver==='undefined')return
   const observer=new IntersectionObserver(entries=>{if(entries.some(e=>e.isIntersecting))onVisible(index)},{root:root.current.closest('.reader-viewport'),rootMargin:'-10% 0px -75% 0px'})
   observer.observe(root.current);return()=>observer.disconnect()
 },[index,onVisible])
 useEffect(()=>{
   if(!near){disposeFrame.current?.();setSrc('');setHtml('');return}
   let cancelled=false;const urls:string[]=[]
   const objectUrl=(blob:Blob)=>{const url=URL.createObjectURL(blob);urls.push(url);return url}
   void (async()=>{
     const blob=await archive.read(path)
     if(cancelled)return
     if(comic){setSrc(objectUrl(blob));return}
     const body=parseXml(await blob.text()).querySelector('body')
     if(!body)throw Error('This chapter has no readable content.')
     const fragment=sanitizeChapter(new XMLSerializer().serializeToString(body))
     for(const image of fragment.querySelectorAll('img')){
       const ref=image.getAttribute('src');image.removeAttribute('src')
       if(!ref)continue
       try{const name=archivePath(path,ref);if(isImage(name)&&archive.entries.has(name)){const imageBlob=await archive.read(name);if(cancelled)return;image.src=objectUrl(imageBlob)}}catch{/* External/missing images are omitted. */}
     }
     for(const link of fragment.querySelectorAll('a'))link.removeAttribute('href')
     const wrapper=document.createElement('div');wrapper.append(fragment)
     if(!cancelled){setHtml(wrapper.innerHTML);onReady(index,0,0)}
   })().catch(e=>{if(!cancelled)setError(e instanceof Error?e.message:'Page could not load.')})
   return()=>{cancelled=true;urls.forEach(URL.revokeObjectURL)}
 },[archive,path,comic,near])
 const attachFrame=()=>{
   disposeFrame.current?.()
   const frame=iframe.current,doc=frame?.contentDocument
   if(!frame||!doc)return
   const resize=()=>{const h=Math.ceil(doc.body?.getBoundingClientRect().height??doc.documentElement.scrollHeight);setHeight(h);frame.style.height=`${h}px`}
   resize()
   doc.addEventListener('dblclick',onCozy)
   const observer=typeof ResizeObserver!=='undefined'?new ResizeObserver(resize):null
   if(doc.body)observer?.observe(doc.body)
   const view=frame.closest<HTMLElement>('.reader-viewport')
   const wheel=(event:WheelEvent)=>{
     if(!view)return
     event.preventDefault()
     if(event.shiftKey){const rect=frame.getBoundingClientRect();const scale=frame.clientWidth?rect.width/frame.clientWidth:1;view.dispatchEvent(new WheelEvent('wheel',{deltaY:event.deltaY,clientX:event.clientX*scale+rect.left,clientY:event.clientY*scale+rect.top,shiftKey:true,bubbles:true,cancelable:true}))}
     else {view.scrollTop+=event.deltaY;view.scrollLeft+=event.deltaX}
   }
   const pointer=(event:PointerEvent)=>{
     if(!view)return
     if(event.type==='pointerdown')event.preventDefault()
     const rect=frame.getBoundingClientRect()
     const scale=frame.clientWidth?rect.width/frame.clientWidth:1
     view.dispatchEvent(new PointerEvent(event.type,{bubbles:true,clientX:event.clientX*scale+rect.left,clientY:event.clientY*scale+rect.top,button:event.button,pointerId:event.pointerId}))
   }
   const click=(event:MouseEvent)=>{
     if(!view)return
     const rect=frame.getBoundingClientRect()
     const scale=frame.clientWidth?rect.width/frame.clientWidth:1
     view.dispatchEvent(new MouseEvent('click',{bubbles:true,clientX:event.clientX*scale+rect.left,clientY:event.clientY*scale+rect.top}))
   }
   doc.addEventListener('wheel',wheel,{passive:false})
   doc.addEventListener('click',click)
   ;['pointerdown','pointermove','pointerup','pointercancel'].forEach(type=>doc.addEventListener(type,pointer as EventListener))
   disposeFrame.current=()=>{observer?.disconnect();doc.removeEventListener('dblclick',onCozy);doc.removeEventListener('wheel',wheel);doc.removeEventListener('click',click);['pointerdown','pointermove','pointerup','pointercancel'].forEach(type=>doc.removeEventListener(type,pointer as EventListener))}

 }
 return <div ref={root} className={`reader-page ${comic?'reader-image-page':'reader-text-page'}`} data-page={index} style={!near||!src&&!html?{minHeight:reserved??'100dvh'}:undefined}>
   {error&&<p role="alert">{error}</p>}
   {comic&&src&&<img draggable={false} src={src} alt={`Page ${index+1}`} onLoad={e=>onReady(index,e.currentTarget.naturalWidth,e.currentTarget.naturalHeight)} onError={()=>setError('This page image is damaged or unsupported by your browser.')}/>}
   {!comic&&html&&<iframe ref={iframe} title={`Book chapter ${index+1}`} sandbox="allow-same-origin" referrerPolicy="no-referrer" srcDoc={chapterDocument(html,settings.fontSize,settings.theme)} onLoad={attachFrame} style={{height:height??'100dvh'}}/>}
 </div>
}
