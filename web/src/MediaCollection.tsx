import { useEffect, useState } from 'react'
import { ArrowLeft, Bookmark, BookmarkCheck, BookOpen, Film, FolderOpen, Headphones, Info, Settings2 } from 'lucide-react'
import { displayTitle, progressLabel, formatSize, groupMedia, subdivisions, type GroupBy, type MediaGroup, type MediaItem } from './media'
type Props = { preserveOrder?: boolean; items: MediaItem[]; compact?: boolean; groupBy?: GroupBy; busy: boolean; onOpen: (item: MediaItem, queue: MediaItem[]) => void; onSave: (item: MediaItem) => void; onMetadata: (item: MediaItem) => void; onGroupMetadata: (item: MediaItem) => void }
export default function MediaCollection({ items, compact, groupBy = 'collection', busy, preserveOrder = false, onOpen, onSave, onMetadata, onGroupMetadata }: Props) {
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
    {!!activeTrail.length && <div className="folder-navigation"><button className="track-play" onClick={() => setTrail(activeTrail.slice(0,-1))}><ArrowLeft size={14}/> Back</button><nav aria-label="Media folders"><button onClick={() => setTrail([])}>All media</button>{activeTrail.map((step,index) => <span key={step.key}> / <button aria-current={index === activeTrail.length-1 ? 'page' : undefined} onClick={() => setTrail(activeTrail.slice(0,index+1))}>{step.label}</button></span>)}</nav>{album?.metadata?.group?.can_edit && <button className="track-play" onClick={() => onGroupMetadata(album!)}><Settings2 size={14}/> Edit album / series</button>}</div>}
    <div className={compact ? 'file-list' : 'catalog-grid'}>{groups.flatMap(group => group.folder ? [(() => {
      const first = group.items.find(item => item.available && item.metadata?.artwork_url) ?? group.items[0]
      const editable = group.items.find(item => item.available && item.metadata?.group?.can_edit)
      const isCollection = groupBy === 'collection' && !activeTrail.length || activeTrail.at(-1)?.stage === 'creator'
      return <article key={`folder:${group.key}`} className={compact ? 'file-row media-folder' : 'catalog-card media-folder'}>
        <button className={compact ? `file-kind ${first.kind} folder-art` : `catalog-cover ${first.kind} folder-art`} onClick={() => openFolder(group)} aria-label={`Open folder: ${group.label}`}>{image(first)}<FolderOpen size={compact ? 20 : 42}/></button>
        <div className={compact ? 'file-copy' : 'catalog-copy'}><button className="folder-title" onClick={() => openFolder(group)}>{group.label}</button><span>{first.source_name}</span><small>{group.items.length} {group.items.length === 1 ? 'item' : 'items'} in this view</small></div>
        <div className="media-actions"><button className="track-play" onClick={() => openFolder(group)}>Open folder</button>{isCollection && editable && <button className="save-media" aria-label={`Edit group: ${group.label}`} title="Edit group cover and tags" onClick={() => onGroupMetadata(editable)}><Settings2 size={17}/></button>}</div>
      </article>
    })()] : group.items.map(item => {
      const Icon = item.kind === 'video' ? Film : item.kind === 'music' ? Headphones : BookOpen
      const action = item.kind === 'video' ? 'Watch' : item.kind === 'music' ? 'Play' : 'Read'
      const title = displayTitle(item), fields = item.metadata?.fields
      return <article key={item.file_key} className={compact ? 'file-row' : 'catalog-card'}>
        <div className={compact ? `file-kind ${item.kind}` : `catalog-cover ${item.kind}`}>
          {image(item)}{compact ? <Icon size={17}/> : <><span>{item.extension.toUpperCase()}</span><Icon size={42} strokeWidth={1.1}/><small>{item.kind === 'book' ? 'BOOK' : item.kind.toUpperCase()}</small></>}
        </div>
        <div className={compact ? 'file-copy' : 'catalog-copy'}><strong title={title}>{title}</strong>{progressLabel(item) && <small className="saved-progress">{progressLabel(item)}</small>}{(fields?.creator || fields?.collection) && <span>{[fields.creator, fields.collection].filter(Boolean).join(' · ')}</span>}<span title={item.relative_path}>{compact ? item.relative_path : item.source_name}</span>{!!fields?.genres?.length && <small className="media-genres">{fields.genres.slice(0, 3).join(' · ')}</small>}{!compact && <small>{item.available ? formatSize(item.size_bytes) : 'Unavailable'} · {item.extension.toUpperCase()}</small>}</div>
        <div className="media-actions"><button className="track-play" disabled={busy || !item.available} onClick={() => onOpen(item, queue)} aria-label={`${action} ${title}`}>{action}</button><button className="save-media" disabled={busy} aria-pressed={item.saved_at !== null} aria-label={`${item.saved_at !== null ? 'Remove from' : 'Save to'} library: ${title}`} title={item.saved_at !== null ? 'Remove from library' : 'Save to library'} onClick={() => onSave(item)}>{item.saved_at !== null ? <BookmarkCheck size={17}/> : <Bookmark size={17}/>}</button><button className="save-media" disabled={!item.available} aria-label={`Metadata: ${title}`} title={item.metadata?.can_edit ? 'Edit metadata' : 'Media details'} onClick={() => onMetadata(item)}><Info size={17}/></button></div>
      </article>
    }))}</div>
  </>
}
