import {displayTitle,type MediaItem} from './media'
export type Playlist={id:string;name:string;tracks:string[]}
export type MusicLibrary={favorites:string[];playlists:Playlist[]}
export type MusicSnapshot={revision:number;library:MusicLibrary}
const normalize=(value:string)=>value.trim().toLowerCase().replace(/\s+/g,' ')
export const artistName=(item:MediaItem)=>item.metadata?.fields.creator?.trim()||'Unknown artist'
export const artistKey=(item:MediaItem)=>`artist:${normalize(artistName(item))}`
export const albumName=(item:MediaItem)=>item.collection_link?.label||item.metadata?.fields.collection?.trim()||'Unknown album'
export const albumKey=(item:MediaItem)=>`album:${JSON.stringify([item.source_id,normalize(artistName(item)),normalize(albumName(item))])}`
export function musicGroups(items:MediaItem[],key:(item:MediaItem)=>string,name:(item:MediaItem)=>string){
 const groups=new Map<string,{key:string;name:string;items:MediaItem[]}>()
 for(const item of items){const id=key(item);let group=groups.get(id);if(!group){group={key:id,name:name(item),items:[]};groups.set(id,group)}group.items.push(item)}
 return [...groups.values()].sort((a,b)=>a.name.localeCompare(b.name,undefined,{numeric:true})||a.key.localeCompare(b.key))
}
export type MusicSort='track'|'title'|'genre'|'artist'|'album'|'duration'|'modified'|'saved'|'source'
export function songsFor(items:MediaItem[],artist:string,album:string,query:string,sort:MusicSort,reverse=false,albumOf=albumKey){
 const value=(item:MediaItem):string|number=>sort==='track'?item.metadata?.fields.track??0:sort==='artist'?artistName(item):sort==='album'?albumName(item):sort==='genre'?item.metadata?.fields.genres?.join(', ')??'':sort==='duration'?item.progress?.total??item.metadata?.fields.duration??0:sort==='modified'?item.modified_at??0:sort==='saved'?item.saved_at??0:sort==='source'?item.source_name:displayTitle(item)
 const search=query.trim().toLowerCase()
 return items.filter(item=>(!artist||artistKey(item)===artist)&&(!album||albumOf(item)===album)&&`${displayTitle(item)} ${artistName(item)} ${albumName(item)} ${(item.metadata?.fields.genres??[]).join(' ')}`.toLowerCase().includes(search)).sort((a,b)=>{const x=value(a),y=value(b);const order=typeof x==='number'&&typeof y==='number'?x-y:String(x).localeCompare(String(y),undefined,{numeric:true});return (order||a.file_key.localeCompare(b.file_key))*(reverse?-1:1)})
}
export const musicDuration=(seconds:number|null|undefined)=>seconds!=null&&Number.isFinite(seconds)?`${Math.floor(seconds/60)}:${String(Math.floor(seconds%60)).padStart(2,'0')}`:'—'
export function playlistId(){const bytes=crypto.getRandomValues(new Uint8Array(16));bytes[6]=(bytes[6]&15)|64;bytes[8]=(bytes[8]&63)|128;const h=Array.from(bytes,b=>b.toString(16).padStart(2,'0')).join('');return `${h.slice(0,8)}-${h.slice(8,12)}-${h.slice(12,16)}-${h.slice(16,20)}-${h.slice(20)}`}
