import { Bookmark, BookmarkCheck, BookOpen, Film, Headphones, Info } from 'lucide-react'
import { displayTitle, formatSize, groupMedia, type GroupBy, type MediaItem } from './media'
type Props = { items: MediaItem[]; compact?: boolean; groupBy?: GroupBy; busy: boolean; onOpen: (item: MediaItem, queue: MediaItem[]) => void; onSave: (item: MediaItem) => void; onMetadata: (item: MediaItem) => void }
export default function MediaCollection({ items, compact, groupBy = 'none', busy, onOpen, onSave, onMetadata }: Props) {
  return <>{groupMedia(items, groupBy).map(group => {
    const queue = group.items.filter(item => item.kind === 'music' && item.available)
    return <div className="media-group" key={group.key}>{group.label && <h3 className="media-group-title">{group.label}<small>{group.items.length} {group.items.length === 1 ? 'item' : 'items'}</small></h3>}<div className={compact ? 'file-list' : 'catalog-grid'}>{group.items.map(item => {
      const Icon = item.kind === 'video' ? Film : item.kind === 'music' ? Headphones : BookOpen
      const action = item.kind === 'video' ? 'Watch' : item.kind === 'music' ? 'Play' : 'Read'
      const title = displayTitle(item), fields = item.metadata?.fields
      return <article key={item.file_key} className={compact ? 'file-row' : 'catalog-card'}>
        <div className={compact ? `file-kind ${item.kind}` : `catalog-cover ${item.kind}`}>
          {item.metadata?.artwork_url && item.available ? <img key={item.metadata.artwork_url} loading="lazy" src={item.metadata.artwork_url} alt="" onError={event => { event.currentTarget.style.display = 'none' }}/>: null}
          {compact ? <Icon size={17}/> : <><span>{item.extension.toUpperCase()}</span><Icon size={42} strokeWidth={1.1}/><small>{item.kind === 'book' ? 'BOOK' : item.kind.toUpperCase()}</small></>}
        </div>
        <div className={compact ? 'file-copy' : 'catalog-copy'}><strong title={title}>{title}</strong>{(fields?.creator || fields?.collection) && <span>{[fields.creator, fields.collection].filter(Boolean).join(' · ')}</span>}<span title={item.relative_path}>{compact ? item.relative_path : item.source_name}</span>{!!fields?.genres?.length && <small className="media-genres">{fields.genres.slice(0, 3).join(' · ')}</small>}{!compact && <small>{item.available ? formatSize(item.size_bytes) : 'Unavailable'} · {item.extension.toUpperCase()}</small>}</div>
        <div className="media-actions"><button className="track-play" disabled={busy || !item.available} onClick={() => onOpen(item, queue)} aria-label={`${action} ${title}`}>{action}</button><button className="save-media" disabled={busy} aria-pressed={item.saved_at !== null} aria-label={`${item.saved_at !== null ? 'Remove from' : 'Save to'} library: ${title}`} title={item.saved_at !== null ? 'Remove from library' : 'Save to library'} onClick={() => onSave(item)}>{item.saved_at !== null ? <BookmarkCheck size={17}/> : <Bookmark size={17}/>}</button><button className="save-media" disabled={!item.available} aria-label={`Metadata: ${title}`} title={item.metadata?.can_edit ? 'Edit metadata' : 'Media details'} onClick={() => onMetadata(item)}><Info size={17}/></button></div>
      </article>
    })}</div></div>
  })}</>
}
