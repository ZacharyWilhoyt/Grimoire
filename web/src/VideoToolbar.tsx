import { useEffect, useRef, useState } from 'react'
import { ListVideo, Captions, AudioLines, Settings, PanelRightClose, Maximize, Minimize, X } from 'lucide-react'
import VideoAudio from './VideoAudio'
import VideoSubtitles from './VideoSubtitles'
import WatchSidebar from './WatchSidebar'
type Props = { video: HTMLVideoElement | null; sourceId: string; path: string; onAudio: (url: string | null) => void; cozy: boolean; onCozy: () => void; fullscreen: boolean; onFullscreen: () => void }
export default function VideoToolbar(p: Props) {
  const [panel, setPanel] = useState(''), [speed, setSpeed] = useState(1), [fit, setFit] = useState('contain'), [away, setAway] = useState(false)
  const root = useRef<HTMLDivElement>(null)
  useEffect(() => { if (p.video) { p.video.playbackRate=speed; p.video.style.objectFit=fit } }, [p.video,speed,fit])
  useEffect(() => { const changed=()=>{if(away && document.hidden) p.video?.pause()}; document.addEventListener('visibilitychange',changed); return()=>document.removeEventListener('visibilitychange',changed) },[away,p.video])
  useEffect(() => {
    const key=(event:KeyboardEvent)=>{if(event.key==='Escape' && panel){event.preventDefault();event.stopPropagation();setPanel('');root.current?.querySelector<HTMLButtonElement>(`[data-panel="${panel}"]`)?.focus()}}
    const outside=(event:PointerEvent)=>{if(event.target instanceof Node && !root.current?.contains(event.target))setPanel('')}
    document.addEventListener('keydown',key,true);document.addEventListener('pointerdown',outside)
    return()=>{document.removeEventListener('keydown',key,true);document.removeEventListener('pointerdown',outside)}
  },[panel])
  const buttons = [['episodes','Episodes preview',ListVideo],['subtitles','Subtitle options',Captions],['audio','Audio options',AudioLines],['settings','Settings',Settings]] as const
  return <div className="video-toolbar" ref={root}>
    <div className="video-option-panel" hidden={panel!=='episodes'}><header>Episodes preview<button aria-label="Close episodes preview" onClick={()=>setPanel('')}><X size={18}/></button></header><WatchSidebar sourceId={p.sourceId} path={p.path} preview/></div>
    <div className="video-option-panel" hidden={panel!=='subtitles'}><header>Subtitle options</header><VideoSubtitles video={p.video} sourceId={p.sourceId} path={p.path}/></div>
    <div className="video-option-panel" hidden={panel!=='audio'}><header>Audio options</header><VideoAudio video={p.video} sourceId={p.sourceId} path={p.path} onSelect={p.onAudio}/></div>
    <div className="video-option-panel player-preferences" hidden={panel!=='settings'}><header>Playback settings</header><label>Video fit<select aria-label="Video fit" value={fit} onChange={e=>setFit(e.target.value)}><option value="contain">Contain</option><option value="cover">Cover</option><option value="fill">Fill</option></select></label><label>Speed<select aria-label="Playback speed" value={speed} onChange={e=>setSpeed(Number(e.target.value))}>{[.5,.75,1,1.25,1.5,1.75,2].map(s=><option key={s} value={s}>{s===1?'Normal':`${s}×`}</option>)}</select></label><label><input type="checkbox" checked={away} onChange={e=>setAway(e.target.checked)}/> Pause when away</label><p>Quality: Original video. CDN selection, automatic intro/outro skipping and video transcoding are not available yet.</p><button onClick={()=>setPanel('audio')}>Audio options</button><button onClick={()=>setPanel('subtitles')}>Subtitle options</button></div>
    <nav aria-label="Video options">{buttons.map(([id,label,Icon])=><button key={id} data-panel={id} title={label} aria-label={label} aria-expanded={panel===id} onClick={()=>setPanel(panel===id?'':id)}><Icon size={23}/></button>)}<button title="Cozy view" aria-label="Cozy view" aria-pressed={p.cozy} onClick={p.onCozy}><PanelRightClose size={23}/></button><button className="toolbar-fullscreen" title={p.fullscreen?'Exit fullscreen':'Fullscreen with subtitles'} aria-label={p.fullscreen?'Exit fullscreen':'Fullscreen with subtitles'} onClick={p.onFullscreen}>{p.fullscreen?<Minimize size={23}/>:<Maximize size={23}/>}</button></nav>
  </div>
}
