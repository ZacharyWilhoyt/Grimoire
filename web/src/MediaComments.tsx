import { useEffect, useId, useRef, useState, type FormEvent } from 'react'
import { Info, MessageSquare, Reply, Trash2 } from 'lucide-react'
import { apiFetch } from './api'

type Comment = { id: string; parent_id: string | null; author: string; reply_to: string | null; body: string; created_at: number; removed: boolean }
type Page = { comments: Comment[]; next: string | null; can_moderate: boolean }

const COMMENT_LIMIT = 2000
const timestampFormat = new Intl.DateTimeFormat(undefined, {
  year: 'numeric', month: 'numeric', day: 'numeric', hour: 'numeric', minute: '2-digit',
})

function CommentTime({ seconds }: { seconds: number }) {
  const timestamp = new Date(seconds * 1000)
  return <time dateTime={timestamp.toISOString()}>{timestampFormat.format(timestamp)}</time>
}

async function commentRequest<T>(url: string, init: RequestInit) {
  const response = await apiFetch(url, init)
  if (!response.ok) {
    const error = await response.json().catch(() => null) as { error?: string } | null
    throw Error(error?.error ?? 'The discussion is unavailable. Try again.')
  }
  return response.json() as Promise<T>
}

function mergeComments(previous: Comment[], incoming: Comment[]) {
  const byId = new Map(previous.map(comment => [comment.id, comment]))
  for (const comment of incoming) {
    if (!byId.get(comment.id)?.removed) byId.set(comment.id, comment)
  }
  return [...byId.values()]
    .sort((a, b) => a.id.length - b.id.length || a.id.localeCompare(b.id))
}

export default function MediaComments({ sourceId, path }: { sourceId: string; path: string }) {
  const id = useId()
  const field = useRef<HTMLTextAreaElement>(null)
  const requestLifetime = useRef<AbortController | null>(null)
  const [comments, setComments] = useState<Comment[]>([])
  const [next, setNext] = useState<string | null>(null)
  const [loaded, setLoaded] = useState(false)
  const [loading, setLoading] = useState(true)
  const [loadError, setLoadError] = useState('')
  const [postError, setPostError] = useState('')
  const [posting, setPosting] = useState(false)
  const [draft, setDraft] = useState('')
  const [reply, setReply] = useState<Comment | null>(null)
  const [canModerate, setCanModerate] = useState(false)
  const [removing, setRemoving] = useState<string | null>(null)
  const [removeError, setRemoveError] = useState('')

  async function load(signal: AbortSignal, after?: string) {
    setLoading(true)
    setLoadError('')
    const query = new URLSearchParams({ source_id: sourceId, path })
    if (after) query.set('after', after)
    try {
      const page = await commentRequest<Page>(`/api/media/comments?${query}`, { signal })
      if (signal.aborted) return
      setComments(previous => mergeComments(previous, page.comments))
      setNext(page.next)
      setCanModerate(page.can_moderate)
      setLoaded(true)
    } catch (error) {
      if (!signal.aborted) setLoadError(error instanceof Error ? error.message : 'Comments could not be loaded.')
    } finally {
      if (!signal.aborted) setLoading(false)
    }
  }

  useEffect(() => {
    const controller = new AbortController()
    requestLifetime.current = controller
    void load(controller.signal)
    return () => controller.abort()
  }, [sourceId, path])

  async function post(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    const signal = requestLifetime.current?.signal
    if (!signal || !loaded || posting || !draft.trim()) return
    setPosting(true)
    setPostError('')
    try {
      const comment = await commentRequest<Comment>('/api/media/comments', {
        method: 'POST', signal, headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ source_id: sourceId, path, body: draft, parent_id: reply?.id }),
      })
      if (signal.aborted) return
      setComments(previous => mergeComments(previous, [comment]))
      setDraft('')
      setReply(null)
    } catch (error) {
      if (!signal.aborted) setPostError(error instanceof Error ? error.message : 'Your comment could not be posted.')
    } finally {
      if (!signal.aborted) setPosting(false)
    }
  }

  async function remove(comment: Comment) {
    const signal = requestLifetime.current?.signal
    if (!signal || removing || !window.confirm('Delete this comment? Its replies will remain.')) return
    setRemoving(comment.id)
    setRemoveError('')
    try {
      const query = new URLSearchParams({ source_id: sourceId, path })
      const response = await apiFetch(`/api/media/comments/${comment.id}?${query}`, { method: 'DELETE', signal })
      if (!response.ok) throw Error('The comment could not be deleted. Check your access and try again.')
      if (signal.aborted) return
      setComments(previous => previous.map(item => item.id === comment.id
        ? { ...item, author: '', body: '[Comment removed]', removed: true }
        : item.parent_id === comment.id ? { ...item, reply_to: 'Removed comment' } : item))
      setReply(previous => previous?.id === comment.id ? null : previous)
    } catch (error) {
      if (!signal.aborted) setRemoveError(error instanceof Error ? error.message : 'The comment could not be deleted.')
    } finally {
      if (!signal.aborted) setRemoving(null)
    }
  }

  const visibleCount = comments.filter(comment => !comment.removed).length
  const removedIds = new Set(comments.filter(comment => comment.removed).map(comment => comment.id))

  return <section className="watch-comments" aria-labelledby={id}>
    <header className="comment-panel-header">
      <h2 id={id}><MessageSquare size={17}/> Comments</h2>
      {loaded && <span className="comment-count" aria-label={`${visibleCount} comments loaded`}>({visibleCount}{next ? '+' : ''})</span>}
    </header>
    <form className="watch-comment-form" onSubmit={post}>
      {reply && <div className="comment-reply-context"><span>Replying to {reply.author}</span><button className="theme-button" type="button" onClick={() => setReply(null)} disabled={posting}>Cancel reply</button></div>}
      <textarea ref={field} aria-label={reply ? 'Write a reply' : 'Write a comment'} placeholder={reply ? 'Write a reply…' : 'Write a comment…'} value={draft} maxLength={COMMENT_LIMIT} disabled={!loaded || posting} onChange={event => setDraft(event.target.value)}/>
      {postError && <p role="alert">{postError}</p>}
      <div className="comment-form-footer">
        <span className="comment-form-note"><Info size={13} aria-hidden="true"/> Plain text</span>
        <span className="comment-character-count" aria-label={`${draft.length} of ${COMMENT_LIMIT} characters`}>{draft.length}/{COMMENT_LIMIT}</span>
        <button className="theme-button theme-button-primary" type="submit" disabled={!loaded || posting || removing === reply?.id || !draft.trim()}>{posting ? 'Posting…' : reply ? 'Post reply' : 'Post'}</button>
      </div>
    </form>
    {removeError && <p role="alert" className="comment-list-status">{removeError}</p>}
    {comments.length > 0 && <ol className="comment-list">
      {comments.map(comment => <li key={comment.id} id={`${id}-${comment.id}`} className={comment.parent_id ? 'comment-reply' : undefined}>
        <span className="comment-avatar" aria-hidden="true">{comment.removed ? '–' : comment.author.charAt(0).toUpperCase()}</span>
        <div className="comment-content">
          <div className="comment-byline"><strong>{comment.removed ? 'Removed comment' : comment.author}</strong><CommentTime seconds={comment.created_at}/></div>
          {comment.parent_id && <a className="comment-parent" href={`#${id}-${comment.parent_id}`} onClick={event => { event.preventDefault(); document.getElementById(`${id}-${comment.parent_id}`)?.scrollIntoView({ block: 'nearest' }) }}>In reply to {removedIds.has(comment.parent_id) ? 'Removed comment' : comment.reply_to}</a>}
          {!comment.removed && <><p>{comment.body}</p><div className="comment-actions">
            <button type="button" className="comment-reply-button" aria-label={`Reply to ${comment.author}`} disabled={posting || removing === comment.id} onClick={() => { setReply(comment); field.current?.focus() }}><Reply size={13} aria-hidden="true"/> Reply</button>
            {canModerate && <button type="button" className="comment-reply-button" aria-label={`Delete comment by ${comment.author}`} disabled={removing !== null || posting} onClick={() => { void remove(comment) }}><Trash2 size={13} aria-hidden="true"/> {removing === comment.id ? 'Deleting…' : 'Delete'}</button>}
          </div></>}
        </div>
      </li>)}
    </ol>}
    {loading && <p role="status" className="comment-list-status">Loading comments…</p>}
    {loaded && !loading && !comments.length && !loadError && <div className="watch-comments-empty"><strong>No comments yet</strong><p>Start the discussion.</p></div>}
    {loadError && <p role="alert" className="comment-list-status">{loadError}</p>}
    {!loading && (next || loadError) && <button className="theme-button comment-more" type="button" onClick={() => { const signal = requestLifetime.current?.signal; if (signal) void load(signal, next ?? undefined) }}>{loadError ? 'Try again' : 'Load more comments'}</button>}
  </section>
}
