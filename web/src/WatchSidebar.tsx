import { useEffect, useState } from 'react'
import { Link as LinkIcon } from 'lucide-react'
import MediaComments from './MediaComments'
import { apiFetch } from './api'
import { displayTitle } from './media'
import { mediaLink, seriesLink, type LinkContext } from './mediaLinks'
export default function WatchSidebar({ sourceId, path, preview = false }: { sourceId: string; path: string; preview?: boolean }) {
  const [context, setContext] = useState<LinkContext | null>(null), [error, setError] = useState('')
  useEffect(() => {
    const controller = new AbortController()
    void apiFetch(`/api/media/link?${new URLSearchParams({ source_id: sourceId, path })}`, { signal: controller.signal }).then(async response => {
      if (!response.ok) throw new Error('Related content is unavailable or access has changed.')
      const data = await response.json() as LinkContext
      if (!controller.signal.aborted) setContext(data)
    }).catch(() => { if (!controller.signal.aborted) setError('Related content is unavailable or access has changed.') })
    return () => controller.abort()
  }, [sourceId, path])
  const group = context?.item?.metadata?.group
  const members = context?.members ?? [], current = members.findIndex(item => item.relative_path === path)
  const next = members[current + 1]
  return <aside className="watch-sidebar"><section className="watch-up-next"><span className="section-kicker">{context?.item?.kind === 'book' ? 'BOOKS & SERIES' : 'EPISODES & SERIES'}</span><h2>{context?.label ?? 'Related content'}</h2><div className="watch-links"><a href={mediaLink(sourceId, path)}><LinkIcon size={13}/> Item link</a>{group && <a href={seriesLink(sourceId, group.id)}><LinkIcon size={13}/> Series link</a>}</div>{next && <a className="watch-next" href={mediaLink(sourceId,next.relative_path)}>Up next: {displayTitle(next)}</a>}{error && <p role="status">{error}</p>}{!context && !error && <p role="status">Loading related content…</p>}{context && members.length <= 1 && <p>No other items in this series.</p>}{members.length > 1 && <nav className="watch-episodes" aria-label="Series contents">{members.map(item => <a key={item.file_key} href={mediaLink(sourceId,item.relative_path)} aria-current={item.relative_path === path ? 'page' : undefined}>{item.metadata?.fields.season != null && <small>S{item.metadata.fields.season} · E{item.metadata.fields.episode ?? '?'}</small>}{displayTitle(item)}</a>)}</nav>}</section>{!preview && <MediaComments key={JSON.stringify([sourceId, path])} sourceId={sourceId} path={path}/>}</aside>
}
