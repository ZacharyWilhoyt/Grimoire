import {usePlayerPreferences} from './playerPreferences'
import { useEffect, useRef, useState } from 'react'
import { SkipForward, ListVideo, Captions, AudioLines, Settings, PanelRightClose, Maximize, Minimize, X, Play, Pause, RotateCcw, RotateCw, Volume2, VolumeX } from 'lucide-react'
import TranscodeOptions, { type TranscodeProfile } from './TranscodeOptions'
import VideoAudio from './VideoAudio'
import VideoSubtitles from './VideoSubtitles'
import WatchSidebar from './WatchSidebar'
type Props = { onNext?:()=>void; onClose?:()=>void; video: HTMLVideoElement | null; sourceId: string; path: string; onAudio: (url: string | null) => void; cozy: boolean; onCozy: () => void; fullscreen: boolean; onFullscreen: () => void }
export default function VideoToolbar(p: Props) {
  const preferences=usePlayerPreferences()
  const [profile,setProfileState]=useState<TranscodeProfile>({height:preferences?.settings.height??0,audio:null})
  const setProfile=(value:TranscodeProfile|((old:TranscodeProfile)=>TranscodeProfile))=>setProfileState(old=>{const next=typeof value==='function'?value(old):value;return next})
  const [visible,setVisible]=useState(true)
  const keyboardFocus=useRef(false)
  const timer=useRef<ReturnType<typeof setTimeout>|undefined>(undefined)
  const [panel, setPanel] = useState(''), [speed, setSpeed] = useState(1), [fit, setFit] = useState('contain'), [away, setAway] = useState(false)
  const [playing,setPlaying]=useState(false),[time,setTime]=useState(0),[duration,setDuration]=useState(0),[volume,setVolume]=useState(1),[muted,setMuted]=useState(false),[playError,setPlayError]=useState('')
  const root = useRef<HTMLDivElement>(null)
  useEffect(()=>{
    const video=p.video;if(!video)return
    const update=()=>{setPlaying(!video.paused&&!video.ended);setTime(video.currentTime||0);setDuration(Number.isFinite(video.duration)?video.duration:0);setVolume(video.volume);setMuted(video.muted)}
    const events=['play','pause','ended','timeupdate','durationchange','loadedmetadata','volumechange']
    update();events.forEach(event=>video.addEventListener(event,update));return()=>events.forEach(event=>video.removeEventListener(event,update))
  },[p.video])
  useEffect(()=>{
    const stage=p.video?.parentElement;if(!stage)return
    const reveal=(event?:Event)=>{if(event?.type==='keydown')keyboardFocus.current=true;if(event?.type.startsWith('pointer'))keyboardFocus.current=false;setVisible(true);clearTimeout(timer.current);if(playing&&!panel&&!(keyboardFocus.current&&root.current?.contains(document.activeElement)))timer.current=setTimeout(()=>{if(!keyboardFocus.current&&document.activeElement instanceof HTMLElement&&root.current?.contains(document.activeElement))document.activeElement.blur();setVisible(false)},2500)}
    const events=['pointermove','pointerdown','keydown','focusin']
    const keyboard=()=>{keyboardFocus.current=true};document.addEventListener('keydown',keyboard,true)
    events.forEach(e=>stage.addEventListener(e,reveal));reveal()
    return()=>{clearTimeout(timer.current);document.removeEventListener('keydown',keyboard,true);events.forEach(e=>stage.removeEventListener(e,reveal))}
  },[p.video,playing,panel])
  useEffect(()=>{const stage=p.video?.parentElement;if(stage)stage.classList.toggle('controls-hidden',!visible);return()=>stage?.classList.remove('controls-hidden')},[visible,p.video])
  const seek=(value:number)=>{if(p.video&&duration>0)p.video.currentTime=Math.min(duration,Math.max(0,value))}
  const stamp=(value:number)=>{const seconds=Math.floor(value);return `${Math.floor(seconds/60)}:${String(seconds%60).padStart(2,'0')}`}
  const play=async()=>{setPlayError('');if(!p.video)return;if(!p.video.paused)p.video.pause();else try{await p.video.play()}catch{setPlayError('Playback could not start. Try again or select a compatible audio track.')}}
  useEffect(() => { if (p.video) { p.video.playbackRate=speed; p.video.style.objectFit=fit } }, [p.video,speed,fit])
  useEffect(() => { const changed=()=>{if(away && document.hidden) p.video?.pause()}; document.addEventListener('visibilitychange',changed); return()=>document.removeEventListener('visibilitychange',changed) },[away,p.video])
  useEffect(() => {
    const key=(event:KeyboardEvent)=>{if(event.key==='Escape' && panel){event.preventDefault();event.stopPropagation();setPanel('');root.current?.querySelector<HTMLButtonElement>(`[data-panel="${panel}"]`)?.focus()}}
    const outside=(event:PointerEvent)=>{if(event.target instanceof Node && !root.current?.contains(event.target))setPanel('')}
    document.addEventListener('keydown',key,true);document.addEventListener('pointerdown',outside)
    return()=>{document.removeEventListener('keydown',key,true);document.removeEventListener('pointerdown',outside)}
  },[panel])
  const buttons = [['episodes','Episodes preview',ListVideo],['subtitles','Subtitle options',Captions],['audio','Audio options',AudioLines],['settings','Settings',Settings]] as const
  return <div className={`video-toolbar ${visible?'':'toolbar-hidden'}`} ref={root}>
    <div className="video-option-panel" hidden={panel!=='episodes'}><header>Episodes preview<button aria-label="Close episodes preview" onClick={()=>setPanel('')}><X size={18}/></button></header><WatchSidebar sourceId={p.sourceId} path={p.path} preview/></div>
    <div className="video-option-panel" hidden={panel!=='subtitles'}><header>Subtitle options</header><VideoSubtitles video={p.video} sourceId={p.sourceId} path={p.path}/></div>
    <div className="video-option-panel" hidden={panel!=='audio'}><header>Audio options</header><VideoAudio onTrack={audio=>setProfile(value=>({...value,audio}))} profile={profile} video={p.video} sourceId={p.sourceId} path={p.path} onSelect={p.onAudio}/></div>
    <div className="video-option-panel player-preferences" hidden={panel!=='settings'}><header>Playback settings</header><label>Video fit<select aria-label="Video fit" value={fit} onChange={e=>setFit(e.target.value)}><option value="contain">Contain</option><option value="cover">Cover</option><option value="fill">Fill</option></select></label><label>Speed<select aria-label="Playback speed" value={speed} onChange={e=>setSpeed(Number(e.target.value))}>{[.5,.75,1,1.25,1.5,1.75,2].map(s=><option key={s} value={s}>{s===1?'Normal':`${s}×`}</option>)}</select></label><label><input type="checkbox" checked={away} onChange={e=>setAway(e.target.checked)}/> Pause when away</label><TranscodeOptions sourceId={p.sourceId} path={p.path} onSelect={p.onAudio} profile={profile} onProfile={setProfile}/><a className="player-credits" href="/licenses/subtitle-renderer.txt" target="_blank" rel="noreferrer">Player credits</a><p>CDN selection and automatic intro/outro skipping are not available yet.</p><button onClick={()=>setPanel('audio')}>Audio options</button><button onClick={()=>setPanel('subtitles')}>Subtitle options</button></div>
    {playError && <p role="status">{playError}</p>}
    <div className="video-timeline"><span>{stamp(time)}</span><input aria-label="Video position" type="range" min="0" max={duration||1} step="0.1" value={Math.min(time,duration)} disabled={!duration} onChange={e=>seek(Number(e.target.value))}/><span>{stamp(duration)}</span></div>
    <div className="video-control-row"><nav aria-label="Playback controls">{p.cozy && p.onClose && <button aria-label="Close video player" title="Back to library" onClick={p.onClose}><X size={23}/></button>}<button aria-label="Back 10 seconds" title="Back 10 seconds" onClick={()=>seek(time-10)}><RotateCcw size={23}/></button><button aria-label={playing?'Pause':'Play'} title={playing?'Pause':'Play'} onClick={()=>{void play()}}>{playing?<Pause size={29}/>:<Play size={29}/>}</button>{p.onNext && <button aria-label="Next episode" title="Next episode" onClick={p.onNext}><SkipForward size={23}/></button>}<button aria-label="Forward 10 seconds" title="Forward 10 seconds" onClick={()=>seek(time+10)}><RotateCw size={23}/></button><button aria-label={muted?'Unmute':'Mute'} onClick={()=>{if(p.video)p.video.muted=!p.video.muted}}>{muted?<VolumeX size={23}/>:<Volume2 size={23}/>}</button><input aria-label="Volume" type="range" min="0" max="1" step="0.05" value={muted?0:volume} onChange={e=>{if(p.video){p.video.volume=Number(e.target.value);p.video.muted=false}}}/></nav>
    <nav aria-label="Video options">{buttons.map(([id,label,Icon])=><button key={id} data-panel={id} title={label} aria-label={label} aria-expanded={panel===id} onClick={()=>setPanel(panel===id?'':id)}><Icon size={23}/></button>)}<button title="Cozy view" aria-label="Cozy view" aria-pressed={p.cozy} onClick={p.onCozy}><PanelRightClose size={23}/></button><button className="toolbar-fullscreen" title={p.fullscreen?'Exit fullscreen':'Fullscreen with subtitles'} aria-label={p.fullscreen?'Exit fullscreen':'Fullscreen with subtitles'} onClick={p.onFullscreen}>{p.fullscreen?<Minimize size={23}/>:<Maximize size={23}/>}</button></nav></div>
  </div>
}
