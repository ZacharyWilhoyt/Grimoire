import { useEffect, useId, useRef, useState, type FormEvent } from 'react'
import { MessageSquare } from 'lucide-react'
import { apiFetch } from './api'

type Comment = { id: string; parent_id: string | null; author: string; reply_to: string | null; body: string; created_at: number }
type Page = { comments: Comment[]; next: string | null }

async function commentRequest<T>(url: string, init: RequestInit) {
  const response = await apiFetch(url, init)
  if (!response.ok) {
    const error = await response.json().catch(() => null) as { error?: string } | null
    throw Error(error?.error ?? 'The discussion is unavailable. Try again.')
  }
  return response.json() as Promise<T>
}

function mergeComments(previous: Comment[], incoming: Comment[]) {
  return [...new Map([...previous, ...incoming].map(comment => [comment.id, comment])).values()]
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

  return <section className="watch-comments" aria-labelledby={id}>
    <h2 id={id}><MessageSquare size={19}/> Comments</h2>
    <form className="watch-comment-form" onSubmit={post}>
      {reply && <div className="comment-reply-context"><span>Replying to {reply.author}</span><button type="button" onClick={() => setReply(null)} disabled={posting}>Cancel reply</button></div>}
      <textarea ref={field} aria-label={reply ? 'Write a reply' : 'Write a comment'} placeholder={reply ? 'Write a reply…' : 'Write a comment…'} value={draft} maxLength={2000} disabled={!loaded || posting} onChange={event => setDraft(event.target.value)}/>
      {postError && <p role="alert">{postError}</p>}
      <button type="submit" disabled={!loaded || posting || !draft.trim()}>{posting ? 'Posting…' : reply ? 'Post reply' : 'Post comment'}</button>
    </form>
    {comments.length > 0 && <ol className="comment-list">
      {comments.map(comment => <li key={comment.id} id={`${id}-${comment.id}`} className={comment.parent_id ? 'comment-reply' : undefined}>
        <div className="comment-byline"><strong>{comment.author}</strong><time dateTime={new Date(comment.created_at * 1000).toISOString()}>{new Date(comment.created_at * 1000).toLocaleString()}</time></div>
        {comment.parent_id && <a className="comment-parent" href={`#${id}-${comment.parent_id}`} onClick={event => { event.preventDefault(); document.getElementById(`${id}-${comment.parent_id}`)?.scrollIntoView({ block: 'nearest' }) }}>In reply to {comment.reply_to}</a>}
        <p>{comment.body}</p>
        <button type="button" className="comment-reply-button" aria-label={`Reply to ${comment.author}`} disabled={posting} onClick={() => { setReply(comment); field.current?.focus() }}>Reply</button>
      </li>)}
    </ol>}
    {loading && <p role="status" className="comment-list-status">Loading comments…</p>}
    {loaded && !loading && !comments.length && !loadError && <div className="watch-comments-empty"><strong>No comments yet</strong><p>Start the discussion.</p></div>}
    {loadError && <p role="alert" className="comment-list-status">{loadError}</p>}
    {!loading && (next || loadError) && <button className="comment-more" type="button" onClick={() => { const signal = requestLifetime.current?.signal; if (signal) void load(signal, next ?? undefined) }}>{loadError ? 'Try again' : 'Load more comments'}</button>}
  </section>
}
