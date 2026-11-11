import { useEffect, useState } from 'react'
import { Copy, RefreshCw, Server, X } from 'lucide-react'
import { apiFetch } from './api'
import { formatSize } from './media'
import { useModalDialog } from './useModalDialog'

export type SourceAgent = { id: string; label: string; online: boolean; revoked: boolean }
export type RemoteSource = { id: string; name: string; is_owner: boolean; available: boolean; node_id?: string | null }
type Item = { id: string; representation_id: string; name: string; extension: string; size_bytes: number }
type Catalog = { items: Item[]; next: string | null; can_access_media: boolean }
export type RemoteBook = { sourceId: string; sourceName: string; item: Item; url: string }

const base = '/api/nodes/v1'

async function load<T>(path: string, init?: RequestInit): Promise<T> {
  const result = await apiFetch(`${base}${path}`, init)
  if (!result.ok) {
    const body = await result.json().catch(() => null)
    throw Error(body?.error ?? 'The source agent request failed.')
  }
  return result.json()
}

export default function SourceAgents({ onClose, onPermissions, onBook, initialAgent, initialSource }: {
  initialAgent?: SourceAgent
  initialSource?: RemoteSource
  onClose: () => void
  onPermissions: (source: { id: string; name: string }) => void
  onBook: (book: RemoteBook) => void
}) {
  const dialog = useModalDialog()
  const origin = window.location.origin
  const [nodes, setNodes] = useState<SourceAgent[]>([])
  const [sources, setSources] = useState<RemoteSource[]>([])
  const [selected, setSelected] = useState<RemoteSource | null>(initialSource ?? null)
  const [catalog, setCatalog] = useState<Catalog | null>(null)
  const [playing, setPlaying] = useState<Item | null>(null)
  const [token, setToken] = useState('')
  const [copied, setCopied] = useState(false)
  const [busy, setBusy] = useState(false)
  const [loading, setLoading] = useState(true)
  const [refresh, setRefresh] = useState(0)
  const [error, setError] = useState('')

  useEffect(() => {
    const controller = new AbortController()
    setLoading(true)
    setError('')
    void Promise.all([
      load<SourceAgent[]>('', { signal: controller.signal }),
      load<RemoteSource[]>('/sources', { signal: controller.signal }),
    ]).then(([nodes, sources]) => {
      if (controller.signal.aborted) return
      setNodes(nodes)
      setSources(sources)
      if (initialAgent) {
        const available = sources.filter(source => source.node_id === initialAgent.id && source.available)
        if (available.length === 1) setSelected(available[0])
      }
    }).catch(error => {
      if (!controller.signal.aborted) setError(error.message)
    }).finally(() => {
      if (!controller.signal.aborted) setLoading(false)
    })
    return () => controller.abort()
  }, [refresh, initialAgent])

  useEffect(() => {
    if (!selected) return
    const controller = new AbortController()
    setCatalog(null)
    setError('')
    void load<Catalog>(`/sources/${selected.id}/catalog`, { signal: controller.signal })
      .then(data => { if (!controller.signal.aborted) setCatalog(data) })
      .catch(error => { if (!controller.signal.aborted) setError(error.message) })
    return () => controller.abort()
  }, [selected, refresh])

  async function issue() {
    setBusy(true); setError(''); setCopied(false)
    try {
      const result = await load<{ token: string }>('/enrollment-tokens', { method: 'POST' })
      setToken(result.token)
    } catch (error) {
      setError(error instanceof Error ? error.message : 'Token creation failed.')
    } finally { setBusy(false) }
  }

  async function revoke(node: SourceAgent) {
    if (!window.confirm(`Revoke ${node.label}? This stops access through the agent.`)) return
    setBusy(true); setError('')
    try {
      const result = await apiFetch(`${base}/${node.id}`, { method: 'DELETE' })
      if (!result.ok) throw Error('Could not revoke this Source Agent.')
      setPlaying(null); setSelected(null); setRefresh(value => value + 1)
    } catch (error) {
      setError(error instanceof Error ? error.message : 'Agent revocation failed.')
    } finally { setBusy(false) }
  }

  async function more() {
    if (!selected || !catalog?.next) return
    setBusy(true); setError('')
    try {
      const page = await load<Catalog>(`/sources/${selected.id}/catalog?after=${catalog.next}`)
      setCatalog({ ...page, items: [...catalog.items, ...page.items] })
    } catch (error) {
      setError(error instanceof Error ? error.message : 'Could not load more items.')
    } finally { setBusy(false) }
  }

  const mediaUrl = (source: RemoteSource, item: Item) => `${base}/sources/${source.id}/representations/${item.representation_id}`

  const listedSources = initialAgent ? sources.filter(source => source.node_id === initialAgent.id) : sources
  const managing = !initialAgent && !initialSource

  return <dialog ref={dialog} className="source-agent-dialog" aria-labelledby="source-agent-heading" onCancel={event => { event.preventDefault(); onClose() }}>
    <header><h2 id="source-agent-heading"><Server size={20}/> {initialAgent?.label ?? initialSource?.name ?? 'Source Agents'}</h2><button className="theme-button theme-button-icon" type="button" aria-label="Close Source Agents" onClick={onClose}><X size={19}/></button></header>
    <div className="source-agent-body">
      {error && <p role="alert" className="catalog-notice error">{error}</p>}
      {managing && <>
      <div className="source-agent-actions"><button className="theme-button theme-button-primary" type="button" onClick={() => { void issue() }} disabled={busy}>Create enrollment token</button><button className="theme-button theme-button-icon" type="button" onClick={() => setRefresh(value => value + 1)} disabled={loading || busy} aria-label="Refresh Source Agents"><RefreshCw size={16}/></button></div>
      <div className="source-agent-token">
        <p>Run setup on the Linux machine that holds your media. It will ask which directories to share and when to create an enrollment token.</p>
        <pre>{`curl -fsSLO ${origin}/setup-source-agent.sh
bash setup-source-agent.sh ${origin}`}</pre>
      </div>
      {token && <div className="source-agent-token"><label>Single-use token <input readOnly value={token} onFocus={event => event.currentTarget.select()}/></label><button className="theme-button" type="button" onClick={() => { if (!navigator.clipboard) { setError('Select and copy the token manually.'); return } void navigator.clipboard.writeText(token).then(() => setCopied(true)).catch(() => setError('Select and copy the token manually.')) }}><Copy size={14}/> {copied ? 'Copied' : 'Copy'}</button><p>Paste the token when the setup script prompts you. It expires in 10 minutes.</p><details><summary>Re-enroll an existing agent</summary><p>Stop the running agent, then use the same Docker volume and a fresh token from the same account. Restart the agent after enrollment.</p><pre>{`docker pull ${window.location.host}/grimoire/source-agent:latest
docker run --rm -it -v grimoire-agent:/var/lib/grimoire-agent ${window.location.host}/grimoire/source-agent:latest enroll ${origin} "My agent"`}</pre></details></div>}
      <h3>Your agents</h3>
      {loading && <p role="status">Checking Source Agents…</p>}
      {!loading && nodes.length === 0 && <p>No agents enrolled yet.</p>}
      <ul className="source-agent-list">{nodes.map(node => <li key={node.id}><span><strong>{node.label}</strong><small>{node.revoked ? 'Revoked' : node.online ? 'Online' : 'Offline'}</small></span>{!node.revoked && <button className="theme-button" type="button" disabled={busy} onClick={() => { void revoke(node) }}>Revoke</button>}</li>)}</ul>
      </>}
      {!managing && loading && <p role="status">Checking remote sources…</p>}
      {!initialSource && <h3>Remote sources</h3>}
      {!initialSource && !loading && listedSources.length === 0 && <p>{initialAgent ? 'This agent has not published a source yet.' : 'No remote sources are available.'}</p>}
      {!initialSource && <ul className="source-agent-list">{listedSources.map(source => <li key={source.id}><button className="theme-button" type="button" disabled={!source.available || busy} onClick={() => { setSelected(source); setCatalog(null); setPlaying(null) }}>{source.name}</button><small>{source.available ? 'Available' : 'Offline or unpublished'}</small>{source.is_owner && <button className="theme-button" type="button" onClick={() => onPermissions({ id: `remote:${source.id}`, name: source.name })}>Permissions</button>}</li>)}</ul>}
      {selected && catalog && <section aria-label={selected.name}>
        <h3>{selected.name}</h3>
        {!catalog.can_access_media && <p>Discovery access only.</p>}
        {playing && (['mp3','flac','m4a','aac','ogg','opus','wav','aiff','wma'].includes(playing.extension)
          ? <audio key={playing.id} controls autoPlay src={mediaUrl(selected, playing)} onError={() => setError('This audio format could not be played by your browser.')}/>
          : <video key={playing.id} controls autoPlay src={mediaUrl(selected, playing)} onError={() => setError('This video format could not be played by your browser.')}/>)}
        <ul className="source-agent-list">{catalog.items.map(item => <li key={item.id}><span><strong>{item.name}</strong><small>{formatSize(item.size_bytes)}</small></span>{catalog.can_access_media && <button className="theme-button" type="button" onClick={() => {
          if (['cbz','epub'].includes(item.extension)) onBook({ sourceId: selected.id, sourceName: selected.name, item, url: mediaUrl(selected, item) })
          else setPlaying(item)
        }}>{['cbz','epub'].includes(item.extension) ? 'Read' : 'Play'}</button>}</li>)}</ul>
        {catalog.next && <button className="theme-button" type="button" disabled={busy} onClick={() => { void more() }}>Load more</button>}
      </section>}
    </div>
  </dialog>
}
