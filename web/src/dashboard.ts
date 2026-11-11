import {orderMembers,type MediaItem} from './media'
export function workKey(item:MediaItem){const fields=item.metadata?.fields;return fields?.collection?.trim()?JSON.stringify([item.kind,fields.collection.trim().toLowerCase(),item.kind==='book'?fields.creator?.trim().toLowerCase()??'':'']):item.file_key}
export function continuation(items:MediaItem[],kind:'video'|'book',catalog:MediaItem[]=[]){
 const seen=new Set<string>(),result:{item:MediaItem;time:number}[]=[]
 const unfinished=items.filter(item=>item.kind===kind&&(item.available||item.remote)&&item.progress&&(!item.progress.completed||kind==='video')&&(item.progress.total==null||item.progress.position<item.progress.total)).sort((a,b)=>(b.progress?.updated_at??0)-(a.progress?.updated_at??0))
 for(const item of unfinished){const key=workKey(item);if(!seen.has(key)){seen.add(key);result.push({item,time:item.progress?.updated_at??0})}}
 const completed=catalog.filter(item=>item.kind===kind&&item.available&&item.metadata?.fields.collection&&item.progress&&((kind==='book'&&item.progress.completed)||item.progress.total!=null&&item.progress.position>=item.progress.total)).sort((a,b)=>(b.progress?.updated_at??0)-(a.progress?.updated_at??0))
 for(const previous of completed){const key=workKey(previous);if(seen.has(key))continue;const members=orderMembers(catalog.filter(item=>item.kind===kind&&item.available&&item.source_id===previous.source_id&&workKey(item)===key));const next=members[members.findIndex(item=>item.file_key===previous.file_key)+1];if(next&&!next.progress?.completed&&(!next.progress?.total||next.progress.position<next.progress.total)){seen.add(key);result.push({item:next,time:previous.progress?.updated_at??0})}}
 return result.sort((a,b)=>b.time-a.time).map(row=>row.item)
}

export function relativeActivity(timestamp:number|undefined){if(!timestamp)return '';const minutes=Math.max(0,Math.floor((Date.now()/1000-timestamp)/60));return minutes<1?'Just now':minutes<60?`${minutes}m ago`:minutes<1440?`${Math.floor(minutes/60)}h ago`:`${Math.floor(minutes/1440)}d ago`}
