import { useEffect, useRef, useState } from 'react'
import {usePlayerPreferences} from './playerPreferences'
import { apiFetch } from './api'
export type TranscodeProfile = { height: number; audio: number | null }
type Quality={height:number;bitrate_kbps:number}
export default function TranscodeOptions({sourceId,path,onSelect,profile,onProfile}:{sourceId:string;path:string;onSelect:(url:string|null)=>void;profile:TranscodeProfile;onProfile:(p:TranscodeProfile)=>void}){
 const preferences=usePlayerPreferences(),restored=useRef(false),alive=useRef(true)
 useEffect(()=>{alive.current=true;return()=>{alive.current=false}},[])
 const [qualities,setQualities]=useState<Quality[]>([]),[error,setError]=useState(''),[busy,setBusy]=useState(false),[actual,setActual]=useState('')
 useEffect(()=>{const abort=new AbortController();void apiFetch(`/api/transcode/qualities?${new URLSearchParams({source_id:sourceId,path})}`,{signal:abort.signal}).then(async r=>{if(!r.ok)throw Error();return r.json()}).then(v=>{if(!abort.signal.aborted)setQualities(v)}).catch(()=>{if(!abort.signal.aborted)setError('Quality details unavailable. Highest supported quality can still be requested.')});return()=>abort.abort()},[sourceId,path])
 async function apply(){setBusy(true);setError('');try{const r=await apiFetch('/api/transcode',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({source_id:sourceId,path,...profile})});const data=await r.json();if(!r.ok)throw Error(data.error??'Playback could not start');if(!alive.current){void apiFetch(`/api/transcode/${encodeURIComponent(data.id)}`,{method:'DELETE'}).catch(()=>{});return}preferences?.update({height:profile.height});setActual(`${data.height}p · target ${(data.bitrate_kbps/1000).toFixed(1)} Mbps video${data.audio_bitrate_kbps?` + ${(data.audio_bitrate_kbps/1000).toFixed(3)} Mbps audio`: " · no audio"}`);onSelect(`/api/transcode/${encodeURIComponent(data.id)}/manifest.m3u8`)}catch(e){setError(e instanceof Error?e.message:'Playback failed')}finally{if(alive.current)setBusy(false)}}
 useEffect(()=>{if(!restored.current){restored.current=true;void apply()}},[])
 const max=qualities.at(-1)
 return <section className="transcode-options"><h3>Playback quality</h3><p>Encoding is managed by the source provider.</p><label>Resolution<select aria-label="Transcoding resolution" value={max&&profile.height<max.height?profile.height:0} onChange={e=>onProfile({...profile,height:Number(e.target.value)})}><option value={0}>Highest available{max?` · ${max.height}p · ${(max.bitrate_kbps/1000).toFixed(1)} Mbps`:''}</option>{qualities.filter(q=>q!==max).map(q=><option key={q.height} value={q.height}>{q.height}p · {(q.bitrate_kbps/1000).toFixed(1)} Mbps</option>)}</select></label><p>Video bitrates are targets; AAC audio adds up to 192 kbps.</p><button disabled={busy} onClick={()=>{void apply()}}>{busy?'Starting…':'Apply quality'}</button>{actual&&<p>Playing: {actual}</p>}{error&&<p role="status">{error}</p>}</section>
}
