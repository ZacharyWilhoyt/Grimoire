import { Bookmark, BookmarkCheck, BookOpen, Film, Headphones } from 'lucide-react'
import { formatSize, type MediaItem } from './media'
type Props = { items: MediaItem[]; compact?: boolean; busy: boolean; onOpen: (item: MediaItem, queue: MediaItem[]) => void; onSave: (item: MediaItem) => void }
export default function MediaCollection({ items, compact, busy, onOpen, onSave }: Props) {
  const queue = items.filter(item => item.kind === 'music' && item.available)
  return <div className={compact ? 'file-list' : 'catalog-grid'}>{items.map(item => {
    const Icon = item.kind === 'video' ? Film : item.kind === 'music' ? Headphones : BookOpen
    const action = item.kind === 'video' ? 'Watch' : item.kind === 'music' ? 'Play' : 'Read'
    return <article key={item.file_key} className={compact ? 'file-row' : 'catalog-card'}>
      {compact ? <span className={`file-kind ${item.kind}`}><Icon size={17}/></span> : <div className={`catalog-cover ${item.kind}`}><span>{item.extension.toUpperCase()}</span><Icon size={42} strokeWidth={1.1}/><small>{item.kind === 'book' ? 'BOOK' : item.kind.toUpperCase()}</small></div>}
      <div className={compact ? 'file-copy' : 'catalog-copy'}><strong title={item.name}>{item.name}</strong><span title={item.relative_path}>{compact ? item.relative_path : item.source_name}</span>{!compact && <small>{item.available ? formatSize(item.size_bytes) : 'Unavailable'} · {item.extension.toUpperCase()}</small>}</div>
      <div className="media-actions"><button className="track-play" disabled={busy || !item.available} onClick={() => onOpen(item, queue)} aria-label={`${action} ${item.name}`}>{action}</button><button className="save-media" disabled={busy} aria-pressed={item.saved_at !== null} aria-label={`${item.saved_at !== null ? 'Remove from' : 'Save to'} library: ${item.name}`} title={item.saved_at !== null ? 'Remove from library' : 'Save to library'} onClick={() => onSave(item)}>{item.saved_at !== null ? <BookmarkCheck size={17}/> : <Bookmark size={17}/>}</button></div>
    </article>
  })}</div>
}
