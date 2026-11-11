import MediaArtwork from './MediaArtwork'
import {apiFetch} from './api'
import { mediaLink, seriesLink } from './mediaLinks'
import { useEffect, useState, type DragEvent } from 'react'
import { ArrowLeft, Bookmark, BookmarkCheck, BookOpen, Film, FolderOpen, Headphones, Info, Link as LinkIcon, Settings2 } from 'lucide-react'
import { displayTitle, progressLabel, formatSize, groupMedia, subdivisions, type GroupBy, type MediaGroup, type MediaItem, type MetadataView } from './media'
type Props = { hideActions?:boolean; onDetails?:(item:MediaItem)=>void; onSeries?:(item:MediaItem)=>void; preserveOrder?: boolean; items: MediaItem[]; compact?: boolean; groupBy?: GroupBy; busy: boolean; onOpen: (item: MediaItem, queue: MediaItem[]) => void; onSave: (item: MediaItem) => void; onMetadata: (item: MediaItem) => void; onGroupMetadata: (item: MediaItem) => void }
export default function MediaCollection({ hideActions=false, items: incoming, compact, groupBy = 'collection', busy, preserveOrder = false, onOpen, onSave, onMetadata, onGroupMetadata, onDetails, onSeries }: Props) {
  const [changes,setChanges]=useState<Record<string,{before:MediaItem['metadata'];after:MetadataView}>>({})
  const items=incoming.map(item=>changes[item.file_key]&&changes[item.file_key].before===item.metadata?{...item,metadata:changes[item.file_key].after}:item)
  const [dragged,setDragged]=useState<MediaItem|null>(null),[over,setOver]=useState(''),[joining,setJoining]=useState(false),[notice,setNotice]=useState(''),[joinError,setJoinError]=useState('')
  const targets=[...new Map(items.filter(item=>item.available&&item.metadata?.group?.can_edit).map(item=>[item.metadata!.group!.id,item])).values()]
  const compatible=(item:MediaItem,target:MediaItem)=>item.available&&item.metadata?.can_edit&&target.metadata?.group?.can_edit&&item.source_id===target.source_id&&item.kind===target.kind&&item.metadata?.group?.id!==target.metadata?.group?.id
  async function join(item:MediaItem,target:MediaItem){
    if(joining||busy||!compatible(item,target))return
    setJoining(true);setJoinError('');setNotice('')
    try{
      const response=await apiFetch(`/api/sources/${encodeURIComponent(item.source_id)}/metadata/group-membership`,{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({path:item.relative_path,target_path:target.relative_path,expected_group_id:target.metadata!.group!.id})})
      const data=await response.json()
      if(!response.ok)throw Error(data.error??'Could not update grouping.')
      setChanges(old=>({...old,[item.file_key]:{before:incoming.find(i=>i.file_key===item.file_key)?.metadata,after:data as MetadataView}}))
      window.dispatchEvent(new CustomEvent('grimoire-media-metadata',{detail:{file_key:item.file_key,metadata:data}}))
      setNotice(`${displayTitle(item)} is now in ${target.metadata!.group!.label}.`)
    }catch(e){setJoinError(e instanceof Error?e.message:'Could not update grouping.')}
    finally{setJoining(false);setDragged(null);setOver('')}
  }
  function drop(event:DragEvent,target:MediaItem){event.preventDefault();setOver('');try{const value=JSON.parse(event.dataTransfer.getData('application/x-grimoire-media'));const item=items.find(i=>i.file_key===value.file_key&&i.source_id===value.source_id);if(item)void join(item,target)}catch{setJoinError('Drag an editable media entry from this library.')}setDragged(null)}
  function dragOver(event:DragEvent,target:MediaItem){if(dragged&&compatible(dragged,target)&&!joining&&!busy){event.preventDefault();event.dataTransfer.dropEffect='move';setOver(target.metadata!.group!.id)}}
  const [trail, setTrail] = useState<{ key: string; label: string; stage: 'creator' | 'collection' | 'part' }[]>([])
  useEffect(() => { setTrail([]) }, [groupBy])
  let visible = items, groups = groupMedia(items, groupBy, preserveOrder)
  let album: MediaItem | undefined
  let valid = true
  for (const step of trail) {
    const selected = groups.find(group => group.key === step.key)
    if (!selected) { valid = false; break }
    visible = selected.items
    if (step.stage === 'creator') groups = groupMedia(visible, 'collection', preserveOrder)
    else if (step.stage === 'collection') { album = visible.find(item => item.available && item.metadata?.group); groups = subdivisions(visible) ?? [{ key: 'files', label: '', items: visible, folder: false }] }
    else groups = [{ key: 'files', label: '', items: visible, folder: false }]
  }
  // Changed filters or metadata can remove an open folder; return to the root.
  useEffect(() => { if (!valid && trail.length) setTrail([]) }, [valid, trail.length])
  const activeTrail = valid ? trail : []
  if (!valid) { groups = groupMedia(items, groupBy, preserveOrder); visible = items; album = undefined }
  const queue = visible.filter(item => item.kind === 'music' && item.available)
  const openFolder = (group: MediaGroup) => {
    const stage = !activeTrail.length && groupBy === 'creator' ? 'creator' : activeTrail.some(step => step.stage === 'collection') ? 'part' : 'collection'
    setTrail([...activeTrail, { key: group.key, label: group.label, stage }])
  }
  const image = (item: MediaItem) => item.metadata?.artwork_url && item.available ? <img key={item.metadata.artwork_url} loading="lazy" src={item.metadata.artwork_url} alt="" onError={event => { event.currentTarget.style.display = 'none' }}/> : null
  return <>
    {targets.length>0&&<p className="metadata-help">Drag editable entries into album / series folders, or use Move to group. Items stay within the same source and media type.</p>}
    {notice&&<p role="status" className="grouping-message">{notice}</p>}{joinError&&<p role="alert" className="grouping-message">{joinError}</p>}{joining&&<p role="status">Updating album / series…</p>}
    {dragged&&activeTrail.length>0&&<div className="group-drop-shelf" aria-label="Drop into album or series">{targets.filter(target=>compatible(dragged,target)).map(target=><div key={target.metadata!.group!.id} className={over===target.metadata!.group!.id?'drop-active':''} onDragOver={event=>dragOver(event,target)} onDrop={event=>drop(event,target)}>Drop into {target.metadata!.group!.label}</div>)}</div>}
    {!!activeTrail.length && <div className="folder-navigation"><button className="track-play" onClick={() => setTrail(activeTrail.slice(0,-1))}><ArrowLeft size={14}/> Back</button><nav aria-label="Media folders"><button onClick={() => setTrail([])}>All media</button>{activeTrail.map((step,index) => <span key={step.key}> / <button aria-current={index === activeTrail.length-1 ? 'page' : undefined} onClick={() => setTrail(activeTrail.slice(0,index+1))}>{step.label}</button></span>)}</nav>{album?.metadata?.group?.can_edit && <button className="track-play" onClick={() => onGroupMetadata(album!)}><Settings2 size={14}/> Edit album / series</button>}</div>}
    <div className={compact ? 'file-list' : 'catalog-grid'}>{groups.flatMap(group => group.folder ? [(() => {
      const first = group.items.find(item => item.available && item.metadata?.artwork_url) ?? group.items[0]
      const editable = group.items.find(item => item.available && item.metadata?.group?.can_edit)
      const isCollection = groupBy === 'collection' && !activeTrail.length || activeTrail.at(-1)?.stage === 'creator'
      const enter=()=>{if(!compact&&isCollection&&onSeries&&first.metadata?.group)onSeries(first);else openFolder(group)}
      return <article key={`folder:${group.key}`} className={`${compact ? 'file-row media-folder' : 'catalog-card media-folder'} media-group-${first.kind} ${over===first.metadata?.group?.id?'drop-active':''}`} onDragOver={event=>{if(isCollection&&editable)dragOver(event,editable)}} onDragLeave={()=>setOver('')} onDrop={event=>{if(isCollection&&editable)drop(event,editable)}}>
        <button className={compact ? `file-kind ${first.kind} folder-art` : `catalog-cover ${first.kind} folder-art`} onClick={enter} aria-label={`Open folder: ${group.label}`}>{compact?<>{image(first)}<FolderOpen size={20}/></>:<MediaArtwork item={first} portrait={first.kind!=='music'}/> }</button>
        <div className={compact ? 'file-copy' : 'catalog-copy'}><button className="folder-title" onClick={enter}>{group.label}</button><span>{first.source_name}</span><small>{group.items.length} {group.items.length === 1 ? 'item' : 'items'} in this view</small></div>
        {!hideActions&&<div className="media-actions">{isCollection && first.metadata?.group && <a className="save-media" aria-label={`Series link: ${group.label}`} href={seriesLink(first.source_id, first.metadata.group.id)}><LinkIcon size={17}/></a>}<button className="track-play" onClick={enter}>Open folder</button>{isCollection && editable && <button className="save-media" aria-label={`Edit group: ${group.label}`} title="Edit group cover and tags" onClick={() => onGroupMetadata(editable)}><Settings2 size={17}/></button>}</div>}
      </article>
    })()] : group.items.map(item => {
      const Icon = item.kind === 'video' ? Film : item.kind === 'music' ? Headphones : BookOpen
      const action = item.kind === 'video' ? 'Watch' : item.kind === 'music' ? 'Play' : 'Read'
      const title = displayTitle(item), fields = item.metadata?.fields
      return <article key={item.file_key} className={compact ? 'file-row' : `catalog-card media-${item.kind}`} onClick={event=>{if(!compact&&!(event.target instanceof Element&&event.target.closest('button,a,input,select,details'))){if(item.kind==='book'||item.remote&&!item.available)(onDetails??onMetadata)(item);else if(item.kind==='video'&&item.available)onOpen(item,[item])}}} draggable={!!item.metadata?.can_edit&&item.available&&!busy&&!joining} onDragStart={event=>{if(!item.metadata?.can_edit||!item.available||busy||joining){event.preventDefault();return}event.dataTransfer.setData('application/x-grimoire-media',JSON.stringify({file_key:item.file_key,source_id:item.source_id}));event.dataTransfer.effectAllowed='move';setDragged(item);setNotice('');setJoinError('')}} onDragEnd={()=>{setDragged(null);setOver('')}}>
        <button disabled={!item.available&&!item.remote} aria-label={`${item.kind==='book'||!item.available?'Details':action} ${title}`} onClick={()=>{if(item.kind==='book'||!item.available)(onDetails??onMetadata)(item);else onOpen(item,queue)}} className={compact ? `file-kind ${item.kind}` : `catalog-cover ${item.kind}`}>
          {compact?<>{image(item)}<Icon size={17}/></>:<MediaArtwork item={item} portrait={item.kind==='book'}/>}
        </button>
        <div className={compact ? 'file-copy' : 'catalog-copy'}>{!compact&&item.kind==='video'&&<small className="episode-identifier">{fields?.episode!=null?`EPISODE ${fields.episode}`:'VIDEO'}</small>}<strong title={title}>{title}</strong>{progressLabel(item) && <small className="saved-progress">{progressLabel(item)}</small>}{(fields?.creator || fields?.collection) && <span>{[fields.creator, fields.collection].filter(Boolean).join(' · ')}</span>}<span title={item.relative_path}>{compact ? item.relative_path : item.source_name}</span>{!!fields?.genres?.length && <small className="media-genres">{fields.genres.slice(0, 3).join(' · ')}</small>}{!compact && <small>{item.available ? formatSize(item.size_bytes) : item.remote_access===false?'Discovery only':'Unavailable'} · {item.extension.toUpperCase()}</small>}</div>
        {!hideActions&&<div className="media-actions">{item.metadata?.can_edit&&item.available&&targets.some(t=>compatible(item,t))&&<details className="grouping-picker"><summary>Move to group</summary><div>{targets.filter(t=>compatible(item,t)).map(target=><button key={target.metadata!.group!.id} disabled={joining||busy} onClick={()=>{void join(item,target)}}>Move to {target.metadata!.group!.label}</button>)}</div></details>}<a className="save-media" aria-label={`Link: ${title}`} href={mediaLink(item.source_id,item.relative_path)}><LinkIcon size={17}/></a>{(!item.remote||item.available)&&<button className="track-play" disabled={busy || !item.available} onClick={() => onOpen(item, queue)} aria-label={`${action} ${title}`}>{action}</button>}<button className="save-media" disabled={busy} aria-pressed={item.saved_at !== null} aria-label={`${item.saved_at !== null ? 'Remove from' : 'Save to'} library: ${title}`} title={item.saved_at !== null ? 'Remove from library' : 'Save to library'} onClick={() => onSave(item)}>{item.saved_at !== null ? <BookmarkCheck size={17}/> : <Bookmark size={17}/>}</button><button className="save-media" disabled={!item.available} aria-label={`Metadata: ${title}`} title={item.metadata?.can_edit ? 'Edit metadata' : 'Media details'} onClick={() => onMetadata(item)}><Info size={17}/></button></div>}
      </article>
    }))}</div>
  </>
}
