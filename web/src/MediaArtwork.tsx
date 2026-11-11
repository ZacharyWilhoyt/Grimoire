import {useState} from 'react'
import {BookOpen,Film,Headphones,Play} from 'lucide-react'
import type {MediaItem} from './media'
export function durationLabel(seconds:number|null|undefined){if(seconds==null||!Number.isFinite(seconds)||seconds<0)return null;return `${Math.floor(seconds/60)}:${String(Math.floor(seconds%60)).padStart(2,'0')}`}
export default function MediaArtwork({item,portrait=false,continuing=false,src}:{item:MediaItem;portrait?:boolean;continuing?:boolean;src?:string|null}){
 const [failed,setFailed]=useState<string[]>([])
 const image=item.available?[src,!portrait?item.metadata?.thumbnail_url:null,item.metadata?.artwork_url].find((url):url is string=>!!url&&!failed.includes(url)):undefined
 const Icon=item.kind==='book'?BookOpen:item.kind==='music'?Headphones:Film
 const duration=durationLabel(item.progress?.total??item.metadata?.fields.duration)
 const ratio=item.progress?.total&&item.progress.total>0?Math.max(0,Math.min(1,item.progress.position/item.progress.total)):null
 const episode=item.metadata?.fields.episode
 return <span className={`media-artwork ${portrait?'artwork-portrait':'artwork-landscape'}`}>
 {image?<img key={image} loading="lazy" src={image} alt="" onError={()=>setFailed(values=>[...values,image])}/>:<Icon className="artwork-placeholder" aria-hidden="true"/>}
 {item.kind==='video'&&!portrait&&duration&&<span className={`artwork-badge ${continuing?'duration-bottom':'duration-top'}`}>{duration}</span>}
 {portrait&&<span className="artwork-badge type-badge">{item.kind==='book'?item.extension.toUpperCase():'SERIES'}</span>}
 {continuing&&item.kind==='book'&&<span className="artwork-badge reading-badge"><BookOpen size={12}/> Reading</span>}
 {episode!=null&&item.kind==='video'&&!portrait&&<span className="artwork-badge episode-badge">{item.metadata?.fields.season!=null?`S${item.metadata.fields.season} · `:''}E{episode}</span>}
 {ratio!=null&&<progress className="artwork-progress" aria-label={item.kind==='book'?'Reading progress':'Playback progress'} max={1} value={ratio}/>}
 {!portrait&&<Play className="artwork-play" aria-hidden="true"/>}
 </span>
}
