import MetadataCredits from './MetadataCredits'
import WatchSidebar from './WatchSidebar'
import {epubChapters,type Chapter} from './epubChapters'
import {ProgressSession} from './progress'
import {displayTitle,type MetadataView,type MediaItem} from './media'
import {useCallback,useEffect,useLayoutEffect,useMemo,useRef,useState} from 'react'
import {ArrowLeft,ChevronLeft,ChevronRight,PanelRightClose,Settings,X} from 'lucide-react'
import {isImage,openArchive,parseXml,type BookArchive} from './bookArchive'
import {readSettings,isSpread,spreadMembers,type ReaderKind} from './readerSettings'
import ReaderPage from './ReaderPage'
import {apiFetch} from './api'
import type {LinkContext} from './mediaLinks'
type Props={userId?:string;sourceId:string;sourceName:string;file:{metadata?:MetadataView|null;name:string;relative_path:string;extension:string};onClose:()=>void;onNext?:(item:MediaItem)=>void}
export default function BookReader({userId,sourceId,sourceName,file,onClose,onNext}:Props){
 const comic=file.extension.toLowerCase()==='cbz'
 const storage=`grimoire:reader:v1:${userId??'local'}`
 const [kind,setKind]=useState<ReaderKind>(()=>{try{const v=localStorage.getItem(`${storage}:kind:${sourceId}:${file.relative_path}`)??localStorage.getItem(`${storage}:last:${comic?'cbz':'epub'}`);return ['Novel','Comic','Manga','Manhua'].includes(v??'')?v as ReaderKind:comic?'Comic':'Novel'}catch{return comic?'Comic':'Novel'}})
 const [settings,setSettings]=useState(()=>readSettings(storage,kind))
 const [cozy,setCozy]=useState(false),[archive,setArchive]=useState<BookArchive|null>(null),[chapters,setChapters]=useState<Chapter[]>([]),[index,setIndex]=useState(0),[loading,setLoading]=useState(true),[error,setError]=useState(''),[progressError,setProgressError]=useState('')
 const [pageSizes,setPageSizes]=useState<Record<number,{width:number;height:number}>>({})
 const wide=useMemo(()=>Object.fromEntries(Object.entries(pageSizes).map(([page,{width,height}])=>[page,isSpread(width,height)])),[pageSizes])
 const [next,setNext]=useState<MediaItem|null>(null)
 const progress=useRef<ProgressSession|null>(null),viewport=useRef<HTMLDivElement>(null),ready=useRef(new Set<number>()),advanceLock=useRef(false)
 const marginTimer=useRef<ReturnType<typeof setTimeout>|undefined>(undefined)
 useEffect(()=>()=>clearTimeout(marginTimer.current),[])
 const drag=useRef<{x:number;y:number;left:number;top:number;moved:boolean}|null>(null),suppressClick=useRef(false)
 const [settingsOpen,setSettingsOpen]=useState(false)
 const settingsButton=useRef<HTMLButtonElement>(null),settingsPanel=useRef<HTMLDivElement>(null)
 const zoomAnchor=useRef<{element:HTMLElement;x:number;y:number;clientX:number;clientY:number}|null>(null)
 useEffect(()=>{
   if(!settingsOpen)return
   const key=(event:KeyboardEvent)=>{if(event.key==='Escape'){setSettingsOpen(false);settingsButton.current?.focus()}}
   const outside=(event:PointerEvent)=>{if(event.target instanceof Node&&!settingsPanel.current?.contains(event.target)&&!settingsButton.current?.contains(event.target))setSettingsOpen(false)}
   document.addEventListener('keydown',key);document.addEventListener('pointerdown',outside)
   return()=>{document.removeEventListener('keydown',key);document.removeEventListener('pointerdown',outside)}
 },[settingsOpen])
 const strip=!comic||settings.layout==='strip'
 useEffect(()=>{try{localStorage.setItem(`${storage}:${kind}`,JSON.stringify(settings));localStorage.setItem(`${storage}:kind:${sourceId}:${file.relative_path}`,kind);localStorage.setItem(`${storage}:last:${comic?'cbz':'epub'}`,kind)}catch{/* Storage is optional. */}},[storage,kind,settings,sourceId,file.relative_path])
  useEffect(() => {
    setProgressError(''); setArchive(null); setChapters([]); setLoading(true)
    const controller = new AbortController()
    let opened: BookArchive | undefined
    const load = async () => {
      const session = new ProgressSession(sourceId, file.relative_path, message => { if (!controller.signal.aborted) setProgressError(message) })
      progress.current = session
      const saved = await session.load()
      if (controller.signal.aborted) return
      opened = await openArchive(`/api/sources/${encodeURIComponent(sourceId)}/book?${new URLSearchParams({ path: file.relative_path })}`, controller.signal, comic?'cbz':'epub')
      if (controller.signal.aborted) { await opened.close(); return }
      let pages: Chapter[]
      if (comic) {
        pages = [...opened.entries.keys()].filter(isImage).sort((a, b) => a.localeCompare(b, undefined, { numeric: true })).map(path => ({ path, label: path }))
      } else {
        const container = parseXml(await (await opened.read('META-INF/container.xml')).text())
        const packagePath = container.querySelector('rootfile')?.getAttribute('full-path')
        if (!packagePath) throw new Error('The EPUB package was not found.')
        const packageXml = parseXml(await (await opened.read(packagePath)).text())
        pages = await epubChapters(opened, packagePath, packageXml)
      }
      if (!pages.length) throw new Error('No readable pages were found in this book.')
      if (!controller.signal.aborted) {
        const located = saved?.locator ? pages.findIndex(page => page.path === saved.locator) : -1
        setIndex(located >= 0 ? located : Math.min(Math.max(0, Math.floor(saved?.position ?? 0)), pages.length - 1))
        setArchive(opened); setChapters(pages); setLoading(false)
      }
    }
    load().catch(error => { if (!controller.signal.aborted) { setError(error instanceof Error ? error.message : 'Could not open this book.'); setLoading(false) } })
    return () => { controller.abort(); void opened?.close().catch(() => {}) }
  }, [sourceId, file.relative_path, comic])
 useEffect(()=>{
   const abort=new AbortController()
   void apiFetch(`/api/media/link?${new URLSearchParams({source_id:sourceId,path:file.relative_path})}`,{signal:abort.signal}).then(async response=>{
     if(!response.ok)return
     const context=await response.json() as LinkContext
     const current=context.members.findIndex(item=>item.relative_path===file.relative_path)
     if(!abort.signal.aborted)setNext(current>=0?context.members[current+1]??null:null)
   }).catch(()=>{})
   return()=>abort.abort()
 },[sourceId,file.relative_path])
 const save=useCallback((page:number)=>{if(ready.current.has(page)&&chapters[page])progress.current?.save({position:page,total:chapters.length,locator:chapters[page].path})},[chapters])
 const [bookFinished,setBookFinished]=useState(false)
 const finishedUnits=useRef(new Set<number>()),currentUnit=useRef(index)
 currentUnit.current=index
 const finishUnit=useCallback((page:number)=>{
   if(!ready.current.has(page)||finishedUnits.current.has(page)||!chapters[page])return
   finishedUnits.current.add(page)
   if(page===chapters.length-1)setBookFinished(true)
   progress.current?.save({position:page,total:chapters.length,locator:chapters[page].path,completed_units:[page],completed:page===chapters.length-1})
 },[chapters])
 const recordPageSize=useCallback((page:number,width:number,height:number)=>{
   setPageSizes(old=>old[page]?.width===width&&old[page]?.height===height?old:{...old,[page]:{width,height}})
 },[])
 const pageReady=useCallback((page:number,width:number,height:number)=>{
   ready.current.add(page)
   if(comic)recordPageSize(page,width,height)
   if(page===index)save(page)
 },[comic,index,recordPageSize,save])
 const visible=useCallback((page:number)=>{if(page>currentUnit.current)finishUnit(currentUnit.current);currentUnit.current=page;setIndex(page);save(page)},[save,finishUnit])
 const members=spreadMembers(index,chapters.length,wide,settings.layout)
 const first=pageSizes[index],second=pageSizes[index+1]
 const spread=members.length===2&&first?.height>0&&second?.height>0&&first.width>0&&second.width>0
 const firstAspect=spread?first.width/first.height:0
 const secondAspect=spread?second.width/second.height:0
 const spreadAspect=firstAspect+secondAspect
 // Use the smaller native height so Original size never enlarges either page at 100%.
 const spreadWidth=spread?Math.min(first.height,second.height)*spreadAspect:0
 const heightFitWidth=`calc(${cozy?'100dvh':'(100dvh - 76px)'} * ${spreadAspect})`
 const fittedSpreadWidth={
   original:`${spreadWidth}px`,
   width:'100cqw',
   height:heightFitWidth,
   both:`min(${spreadWidth}px, 100cqw, ${heightFitWidth})`,
 }[settings.fit]
 const preload=useRef<Promise<void>>(Promise.resolve())
 const spreadEnd=index+members.length
 useEffect(()=>{
   if(!archive||!comic)return
   const first=Math.max(0,index-2),end=Math.min(chapters.length,spreadEnd+2)
   archive.retain(new Set(chapters.slice(first,end).map(page=>page.path)))
   let cancelled=false
   // Keep one extraction slot free for a page the reader opens on demand.
   preload.current=preload.current.then(async()=>{
     for(const page of [spreadEnd,spreadEnd+1,index-1,index-2]){
       if(cancelled)return
       if(chapters[page])await archive.read(chapters[page].path).catch(()=>{})
     }
   })
   return()=>{cancelled=true}
 },[archive,chapters,comic,index,spreadEnd])
 // Decode only the next candidate to decide whether two portrait pages can pair.
 useEffect(()=>{
   if(!archive||!comic||settings.layout!=='double'||!chapters[index+1]||wide[index+1]!==undefined)return
   let cancelled=false,url:string|undefined
   void archive.read(chapters[index+1].path).then(blob=>{
     if(cancelled)return
     url=URL.createObjectURL(blob);const image=new Image()
     image.onload=()=>{if(!cancelled)recordPageSize(index+1,image.naturalWidth,image.naturalHeight);if(url)URL.revokeObjectURL(url)}
     image.onerror=()=>{if(url)URL.revokeObjectURL(url)};image.src=url
   }).catch(()=>{})
   return()=>{cancelled=true;if(url)URL.revokeObjectURL(url)}
 },[archive,chapters,index,comic,settings.layout,wide,recordPageSize])
 const advance=()=>{
   finishUnit(index)
   if(index+members.length<chapters.length){setIndex(index+members.length);if(viewport.current)viewport.current.scrollTop=0}
   else if(next&&onNext&&!advanceLock.current){advanceLock.current=true;onNext(next)}
 }
 const previous=()=>{
   let previous=index-1
   if(settings.layout==='double'&&previous>0&&wide[previous]===false&&wide[previous-1]===false)previous--
   setIndex(Math.max(0,previous));if(viewport.current)viewport.current.scrollTop=0
 }
 const go=(page:number)=>{if(page===index+1)finishUnit(index);setIndex(page);if(strip)viewport.current?.querySelector(`[data-page="${page}"]`)?.scrollIntoView({block:'start'});else if(viewport.current)viewport.current.scrollTop=0}
 useEffect(()=>{if(strip&&!loading)requestAnimationFrame(()=>viewport.current?.querySelector(`[data-page="${index}"]`)?.scrollIntoView?.({block:'start'}))},[loading,strip])
 useEffect(()=>{
   const view=viewport.current;if(!view)return
   const wheel=(event:WheelEvent)=>{
     if(!event.shiftKey)return
     event.preventDefault()
     const element=view.querySelector<HTMLElement>('.reader-spread')??[...view.querySelectorAll<HTMLElement>('.reader-page img,.reader-page iframe')].find(element=>{const rect=element.getBoundingClientRect();return event.clientX>=rect.left&&event.clientX<=rect.right&&event.clientY>=rect.top&&event.clientY<=rect.bottom})??view.querySelector<HTMLElement>('.reader-surface')
     if(element){const rect=element.getBoundingClientRect();zoomAnchor.current={element,x:rect.width?(event.clientX-rect.left)/rect.width:0,y:rect.height?(event.clientY-rect.top)/rect.height:0,clientX:event.clientX,clientY:event.clientY}}
     setSettings(old=>({...old,zoom:Math.max(25,Math.min(400,old.zoom+(event.deltaY<0?10:-10)))}))
   }
   view.addEventListener('wheel',wheel,{passive:false});return()=>view.removeEventListener('wheel',wheel)
 },[])
 useLayoutEffect(()=>{
   const anchor=zoomAnchor.current,view=viewport.current
   zoomAnchor.current=null
   if(!anchor||!view||!anchor.element.isConnected)return
   const rect=anchor.element.getBoundingClientRect()
   view.scrollLeft+=rect.left+rect.width*anchor.x-anchor.clientX
   view.scrollTop+=rect.top+rect.height*anchor.y-anchor.clientY
 },[settings.zoom])
 const toggleCozy=useCallback(()=>{clearTimeout(marginTimer.current);setCozy(value=>!value)},[])
 return <section className={`watch-page book-watch reader-revamp ${cozy?'watch-cozy':''}`} aria-label="Book reading page">
 <main className="watch-main">
 <header className="watch-header"><button className="watch-back" aria-label="Close reader" onClick={onClose}><ArrowLeft/> Back</button><div className="reader-heading"><h1>{displayTitle(file)}</h1><span>{sourceName}</span></div>
 <nav className="reader-navigation" aria-label="Page navigation">
 <button aria-label="Previous" disabled={loading||index===0} onClick={previous}><ChevronLeft/></button>
 <label>{comic?'Page':'Chapter'}<select aria-label={comic?'Page':'Chapter'} value={index} disabled={loading} onChange={e=>go(Number(e.target.value))}>{chapters.map((chapter,i)=><option key={chapter.path} value={i}>{comic?i+1:chapter.label}</option>)}</select> / {chapters.length}</label>
 <button aria-label={index===chapters.length-1&&!next?'Finish book':'Next'} title={index===chapters.length-1&&!next?'Mark book finished':'Next'} disabled={loading||index>=chapters.length-1&&!next&&bookFinished} onClick={()=>strip&&index<chapters.length-1?go(index+1):advance()}><ChevronRight/></button>
 </nav><button className="reader-cozy-toggle" aria-label="Cozy reading view" aria-pressed={cozy} onClick={toggleCozy}><PanelRightClose/></button></header>
 <div className="reader-layout">
 <button ref={settingsButton} className="reader-settings-toggle" aria-label="Reading settings" aria-expanded={settingsOpen} aria-controls="reading-settings" onClick={()=>setSettingsOpen(value=>!value)}><Settings size={21}/></button>
 {settingsOpen&&<div ref={settingsPanel} className="reader-settings-panel" id="reading-settings" role="region" aria-label="Reading settings panel"><header>Reading settings<button aria-label="Close reading settings" onClick={()=>{setSettingsOpen(false);settingsButton.current?.focus()}}><X size={18}/></button></header>
 <label>Content type<select aria-label="Content type" value={kind} onChange={e=>{const value=e.target.value as ReaderKind;setKind(value);setSettings(readSettings(storage,value))}}>{['Novel','Comic','Manga','Manhua'].map(value=><option key={value} value={value}>{value === 'Manhua' ? 'Manhwa' : value}</option>)}</select></label>
 {comic&&<><label>Fit<select aria-label="Page fit" value={settings.fit} onChange={e=>setSettings({...settings,fit:e.target.value as typeof settings.fit})}><option value="original">Original size</option><option value="width">Fit width</option><option value="height">Fit height</option><option value="both">Fit both</option></select></label><label>Layout<select aria-label="Page layout" value={settings.layout} onChange={e=>setSettings({...settings,layout:e.target.value as typeof settings.layout})}><option value="single">Single page</option><option value="double">Double page</option><option value="strip">Long strip</option></select></label><label>Reading direction<select aria-label="Reading direction" value={settings.direction} onChange={e=>setSettings({...settings,direction:e.target.value as 'ltr'|'rtl'})}><option value="ltr">Left to right</option><option value="rtl">Right to left</option></select></label></>}

 {!comic&&<><label>Text size<input aria-label="Text size" type="range" min={14} max={30} value={settings.fontSize} onChange={e=>setSettings({...settings,fontSize:Number(e.target.value)})}/></label><label>Theme<select aria-label="Reading theme" value={settings.theme} onChange={e=>setSettings({...settings,theme:e.target.value as 'paper'|'night'})}><option value="paper">Paper</option><option value="night">Night</option></select></label></>}
 <MetadataCredits credits={file.metadata?.credits??[]}/>
 <p>Double click content for cozy view. Shift + scroll to zoom; drag to pan.</p>
 </div>}
 <div ref={viewport} className={`reader-viewport fit-${settings.fit} ${strip?'reader-strip':'reader-paged'}`} tabIndex={0} aria-label="Reading content"
 onDoubleClick={toggleCozy}
 onPointerDown={e=>{if(e.button!==0||e.target instanceof Element&&e.target.closest('button,a'))return;drag.current={x:e.clientX,y:e.clientY,left:e.currentTarget.scrollLeft,top:e.currentTarget.scrollTop,moved:false}}}
 onPointerMove={e=>{const start=drag.current;if(!start)return;if(Math.abs(e.clientX-start.x)+Math.abs(e.clientY-start.y)>5){start.moved=true;if(e.isTrusted)e.currentTarget.setPointerCapture?.(e.pointerId);e.currentTarget.scrollLeft=start.left-(e.clientX-start.x);e.currentTarget.scrollTop=start.top-(e.clientY-start.y)}}}
 onPointerUp={()=>{suppressClick.current=drag.current?.moved??false;drag.current=null}}
 onPointerCancel={()=>{drag.current=null}}
 onClick={e=>{if(suppressClick.current){suppressClick.current=false;return}if(e.target instanceof Element&&e.target.closest('button,a'))return;const rect=e.currentTarget.getBoundingClientRect(),x=e.clientX-rect.left,margin=Math.max(40,rect.width*.12);if(x<margin||x>rect.width-margin){clearTimeout(marginTimer.current);const forward=x>rect.width-margin;marginTimer.current=setTimeout(()=>{if(strip){if(forward&&index<chapters.length-1)go(index+1);else if(forward)advance();else go(Math.max(0,index-1))}else if(forward)advance();else previous()},220)}}}
 onScroll={e=>{const v=e.currentTarget;if(strip&&v.scrollTop>20&&v.scrollTop+v.clientHeight>=v.scrollHeight-3&&ready.current.has(chapters.length-1)&&!advanceLock.current){finishUnit(chapters.length-1);if(next){advanceLock.current=true;onNext?.(next)}}}}>
 {loading&&<p role="status">Opening book content…</p>}{error&&<p role="alert">{error}</p>}{progressError&&<p role="status">{progressError}</p>}
 {archive&&!loading&&!error&&<div className={spread?'reader-surface reader-spread':'reader-surface'} style={{
   ...(spread?{width:`calc(${fittedSpreadWidth} * ${settings.zoom/100})`,gridTemplateColumns:`minmax(0, ${100*firstAspect/spreadAspect}fr) minmax(0, ${100*secondAspect/spreadAspect}fr)`}:{zoom:settings.zoom/100}),
   direction:comic&&settings.layout==='double'?settings.direction:'ltr'}}>{(strip?chapters.map((_,i)=>i):members).map(page=><ReaderPage key={chapters[page].path} archive={archive} path={chapters[page].path} index={page} comic={comic} settings={settings} onReady={pageReady} onVisible={strip?visible:undefined} onCozy={toggleCozy} lazy={strip&&page!==index}/>)}</div>}
 </div></div></main>
 {!cozy&&<div className="reader-related"><WatchSidebar sourceId={sourceId} path={file.relative_path}/></div>}
 </section>
}
