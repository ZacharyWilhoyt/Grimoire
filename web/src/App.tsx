import { linkRequest, mediaLink, type LinkContext } from './mediaLinks'
import ConnectDirectory from './ConnectDirectory'
import { loadCatalogPage, type PersonalPage } from './catalogLoader'
import GroupEditor from './GroupEditor'
import ArrConnections from './ArrConnections'
import MetadataCredits from './MetadataCredits'
import MetadataEditor from './MetadataEditor'
import SourceMetadataTools from './SourceMetadataTools'
import MetadataFilters from './MetadataFilters'
import CatalogViews from './CatalogViews'
import MediaCollection from './MediaCollection'
import { filterMedia, pageFromHash, pageHashes, type MediaItem, type PersonalState, type PageName, type Sort, type GroupBy } from './media'
import SourcePermissions, { policyLabel, type AcquisitionPolicy } from './SourcePermissions'
import AccountSettings from './AccountSettings'
import Authentication, { type User } from './Authentication'
import { apiFetch } from './api'
import BookReader from './BookReader'
import VideoPlayer from './VideoPlayer'
import MusicPlayer from './MusicPlayer'
import { useEffect, useRef, useState, type FormEvent } from 'react'
import { BookOpen, ChevronDown, Compass, Film, FolderOpen, Headphones, Library, Menu, Plus, Search, Settings2, ShieldCheck, RefreshCw, Sparkles, Trash2 } from 'lucide-react'

type MediaKind = 'All' | 'Video' | 'Music' | 'Books'
type Source = { id: string; name: string; available: boolean; is_owner: boolean; can_access_media: boolean; acquisition_policy: AcquisitionPolicy }
async function readApi<T>(url: string, signal?: AbortSignal): Promise<T> {
  const response = await apiFetch(url, { signal })
  const data = await response.json()
  if (!response.ok) throw new Error(data.error ?? 'Could not load your media.')
  return data as T
}

function Workspace({ user, logout }: { user: User; logout: () => void }) {
  const [linkedSeries, setLinkedSeries] = useState<LinkContext | null>(null)
  const [linkLoading, setLinkLoading] = useState(false)
  const [linkError, setLinkError] = useState('')
  const [groupItem, setGroupItem] = useState<MediaItem | null>(null)
  const [metadataItem, setMetadataItem] = useState<MediaItem | null>(null)
  const [genre, setGenre] = useState('')
  const [groupBy, setGroupBy] = useState<GroupBy>('collection')
  const [permissionsSource, setPermissionsSource] = useState<Source | null>(null)
  const [sourceRefresh, setSourceRefresh] = useState(0)
  const [sourcesError, setSourcesError] = useState('')
  const [settingsOpen, setSettingsOpen] = useState(false)
  const [reading, setReading] = useState<MediaItem | null>(null)
  const [video, setVideo] = useState<MediaItem | null>(null)
  const [playback, setPlayback] = useState<{ tracks: MediaItem[]; index: number } | null>(null)
  const [filter, setFilter] = useState<MediaKind>('All')
  const [query, setQuery] = useState('')
  const [page, setPage] = useState<PageName>(() => pageFromHash(window.location.hash))
  const [catalog, setCatalog] = useState<MediaItem[]>([])
  const [saved, setSaved] = useState<MediaItem[]>([])
  const [continuing, setContinuing] = useState<MediaItem[]>([])
  const [recent, setRecent] = useState<MediaItem[]>([])
  const [pageLoading, setPageLoading] = useState(true)
  const [pageError, setPageError] = useState('')
  const [pageNote, setPageNote] = useState('')
  const [actionError, setActionError] = useState('')
  const [actionBusy, setActionBusy] = useState(false)
  const [hasMore, setHasMore] = useState(false)
  const [nextOffset, setNextOffset] = useState(0)
  const [sourceFilter, setSourceFilter] = useState('')
  const [sort, setSort] = useState<Sort>(() => pageFromHash(window.location.hash) === 'My library' ? 'saved' : 'title')
  const search = useRef<HTMLInputElement>(null)
  const [menuOpen, setMenuOpen] = useState(false)
  const [apiStatus, setApiStatus] = useState<'checking' | 'connected' | 'offline'>('checking')
  const [sources, setSources] = useState<Source[]>([])
  const [selectedSource, setSelectedSource] = useState<Source | null>(null)
  const [sourceFiles, setSourceFiles] = useState<MediaItem[]>([])
  const [sourceLoading, setSourceLoading] = useState(false)
  const [sourceError, setSourceError] = useState('')
  const [connectOpen, setConnectOpen] = useState(false)
  const [directoryPath, setDirectoryPath] = useState('')
  const [connectError, setConnectError] = useState('')
  const [connecting, setConnecting] = useState(false)

  useEffect(() => {
    const controller = new AbortController()
    setSourcesError('')
    apiFetch('/api/sources', { signal: controller.signal }).then(async response => {
      const data = await response.json()
      if (!response.ok) throw new Error(data.error ?? 'Could not load sources.')
      if (controller.signal.aborted) return
      const loaded = data as Source[]
      setApiStatus('connected'); setSources(loaded)
      setSourceFilter(current => loaded.some(source => source.id === current && source.can_access_media) ? current : '')
      setSelectedSource(current => current ? loaded.find(source => source.id === current.id) ?? null : null)
      const allowed = (id: string) => loaded.some(source => source.id === id && source.can_access_media)
      setPlayback(current => current && allowed(current.tracks[current.index].source_id) ? current : null)
      setVideo(current => current && allowed(current.source_id) ? current : null)
      setReading(current => current && allowed(current.source_id) ? current : null)
    }).catch(error => { if (!controller.signal.aborted) { setApiStatus('offline'); setSourcesError(error instanceof Error ? error.message : 'Could not load sources.') } })
    return () => controller.abort()
  }, [sourceRefresh])

  useEffect(() => {
    setSourceFiles([]); setSourceError(''); setSourceLoading(false)
    if (!selectedSource?.can_access_media) return
    const controller = new AbortController()
    setSourceLoading(true)
    apiFetch(`/api/sources/${encodeURIComponent(selectedSource.id)}/media`, { signal: controller.signal })
      .then(async response => {
        const data = await response.json()
        if (!response.ok) throw new Error(data.error ?? 'Could not scan this directory.')
        if (!controller.signal.aborted) setSourceFiles(data as MediaItem[])
      })
      .catch(error => { if (!controller.signal.aborted) setSourceError(error instanceof Error ? error.message : 'Could not scan this directory.') })
      .finally(() => { if (!controller.signal.aborted) setSourceLoading(false) })
    return () => controller.abort()
  }, [selectedSource])

  const kind = filter === 'Video' ? 'video' : filter === 'Music' ? 'music' : filter === 'Books' ? 'book' : ''
  const filteredFiles = filterMedia(sourceFiles, query, kind, '', 'title', genre)
  const setKind = (kind: string) => setFilter(kind === 'video' ? 'Video' : kind === 'music' ? 'Music' : kind === 'book' ? 'Books' : 'All')
  const navigate = (next: PageName) => {
    setPage(next); setSelectedSource(null); setQuery(''); setFilter('All'); setGenre(''); setSourceFilter(''); setSort(next === 'My library' ? 'saved' : 'title'); setMenuOpen(false); setActionError('')
    window.history.pushState(null, '', `/${pageHashes[next]}`)
  }
  useEffect(() => {
    const changed = () => { const next = pageFromHash(window.location.hash); setPage(next); setSelectedSource(null); setQuery(''); setFilter('All'); setGenre(''); setSourceFilter(''); setSort(next === 'My library' ? 'saved' : 'title') }
    const keyboard = (event: KeyboardEvent) => {
      if (event.defaultPrevented || event.altKey || !(event.ctrlKey || event.metaKey) || event.key.toLowerCase() !== 'k') return
      if (document.querySelector('dialog[open], .modal-backdrop')) return
      const target = event.target
      if (target instanceof Element && target !== search.current && target.closest('input, textarea, select, [contenteditable]:not([contenteditable="false"]), [role="textbox"]')) return
      if (search.current) { event.preventDefault(); search.current.focus() }
    }
    window.addEventListener('hashchange', changed); window.addEventListener('keydown', keyboard)
    return () => { window.removeEventListener('hashchange', changed); window.removeEventListener('keydown', keyboard) }
  }, [])

  useEffect(() => {
    if (selectedSource) return
    const controller = new AbortController()
    setPageLoading(true); setPageError(''); setPageNote(''); setHasMore(false)
    if (page === 'My library') setSaved([])
    else if (page === 'Discover') setCatalog([])
    else { setContinuing([]); setRecent([]); setSaved([]); setCatalog([]) }
    const load = async () => {
      const notes: string[] = []
      const result = await loadCatalogPage(page, controller.signal, readApi)
      if (controller.signal.aborted) return
      if (result.continuing) {
        setContinuing(result.continuing.items)
        if (result.continuing.unavailable_items) notes.push('Some items with saved progress are unavailable.')
      }
      if (result.recent) {
        setRecent(result.recent.items)
        if (result.recent.unavailable_items) notes.push('Some recently opened files are currently unavailable.')
      }
      if (result.library) {
        const library = result.library
        setSaved(library.items); setHasMore(library.has_more); setNextOffset(library.next_offset)
        if (library.unavailable_items) notes.push('Some saved files are currently unavailable. Their saved records are retained.')
      }
      if (result.catalog) {
        const catalog = result.catalog
        setCatalog(catalog.items)
        if (catalog.truncated) notes.push('This catalog preview is limited to 5,000 files from 32 sources. Browse a source directly for additional files.')
        if (catalog.unavailable_sources) notes.push(`${catalog.unavailable_sources} accessible sources could not be scanned.`)
      }
      setPageError(result.errors.join(' '))
      if (!controller.signal.aborted) setPageNote(notes.join(' '))
    }
    load().catch(error => { if (!controller.signal.aborted) setPageError(error instanceof Error ? error.message : 'Could not load media.') }).finally(() => { if (!controller.signal.aborted) setPageLoading(false) })
    return () => controller.abort()
  }, [page, sourceRefresh, selectedSource?.id])

  const patchPersonal = (item: MediaItem, personal: PersonalState) => {
    const updated = { ...item, ...personal }
    const patch = (items: MediaItem[]) => items.map(old => old.file_key === item.file_key ? { ...old, ...personal } : old)
    setCatalog(patch); setSourceFiles(patch)
    setSaved(items => personal.saved_at === null ? items.filter(old => old.file_key !== item.file_key) : [updated, ...items.filter(old => old.file_key !== item.file_key)])
    if (personal.last_opened_at !== null) setRecent(items => [updated, ...items.filter(old => old.file_key !== item.file_key)])
  }
  const openMedia = async (item: MediaItem, queue: MediaItem[], updateUrl = true, signal?: AbortSignal) => {
    setActionBusy(true); setActionError('')
    try {
      const response = await apiFetch('/api/activity/open', { signal, method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ source_id: item.source_id, relative_path: item.relative_path }) })
      const data = await response.json()
      if (!response.ok) throw new Error(data.error ?? 'Could not open this media.')
      if (signal?.aborted) return
      patchPersonal(item, data as PersonalState)
      setReading(null); setVideo(null); setPlayback(null); setLinkedSeries(null)
      if (updateUrl) window.history.pushState(null, '', mediaLink(item.source_id, item.relative_path))
      if (item.kind === 'book') setReading(item)
      else if (item.kind === 'video') setVideo(item)
      else { const tracks = queue.some(track => track.file_key === item.file_key) ? queue : [item]; setPlayback({ tracks, index: tracks.findIndex(track => track.file_key === item.file_key) }) }
    } catch (error) { if (signal?.aborted) return; setActionError(error instanceof Error ? error.message : 'Could not open media.'); setPlayback(null) }
    finally { setActionBusy(false) }
  }
  useEffect(() => {
    let controller = new AbortController()
    const load = () => {
      controller.abort(); controller = new AbortController()
      const signal = controller.signal
      const url = linkRequest(window.location.search)
      setVideo(null); setReading(null); setPlayback(null); setLinkedSeries(null); setLinkError('')
      if (!url) { setLinkLoading(false); return }
      setLinkLoading(true)
      void readApi<LinkContext>(url, signal).then(async context => {
        if (signal.aborted) return
        if (context.item) await openMedia(context.item, context.members, false, signal)
        else setLinkedSeries(context)
      }).catch(error => { if (!signal.aborted) setLinkError(error instanceof Error ? error.message : 'This link is unavailable.') }).finally(() => { if (!signal.aborted) setLinkLoading(false) })
    }
    load(); window.addEventListener('popstate', load)
    return () => { controller.abort(); window.removeEventListener('popstate', load) }
    // Resolve URLs only on entry/browser navigation. Ordinary item selection updates history directly.
  }, [])
  const closeWatch = () => { setVideo(null); setReading(null); setLinkedSeries(null); setLinkError(''); window.history.pushState(null, '', '/#discover') }
  const toggleSaved = async (item: MediaItem) => {
    setActionBusy(true); setActionError('')
    try {
      const response = item.saved_at !== null
        ? await apiFetch(`/api/library/${item.file_key}`, { method: 'DELETE' })
        : await apiFetch('/api/library', { method: 'PUT', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ source_id: item.source_id, relative_path: item.relative_path }) })
      if (!response.ok) { const data = await response.json(); throw new Error(data.error ?? 'Could not update your library.') }
      patchPersonal(item, item.saved_at !== null ? { file_key: item.file_key, saved_at: null, last_opened_at: item.last_opened_at } : await response.json() as PersonalState)
    } catch (error) { setActionError(error instanceof Error ? error.message : 'Could not update your library.') }
    finally { setActionBusy(false) }
  }
  const loadMore = async () => {
    setActionBusy(true); setPageError('')
    try {
      const next = await readApi<PersonalPage>(`/api/library?offset=${nextOffset}`)
      setSaved(items => [...items, ...next.items.filter(item => !items.some(old => old.file_key === item.file_key))]); setHasMore(next.has_more); setNextOffset(next.next_offset)
    } catch (error) { setPageError(error instanceof Error ? error.message : 'Could not load saved items.') }
    finally { setActionBusy(false) }
  }
  const connectDirectory = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault()
    setConnecting(true)
    setConnectError('')
    try {
      const response = await apiFetch('/api/sources', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ path: directoryPath }) })
      const result = await response.json()
      if (!response.ok) throw new Error(result.error ?? 'Could not connect that directory.')
      const source = result as Source
      setSources(current => [...current, source])
      setSelectedSource(source)
      setFilter('All')
      setQuery('')
      setConnectOpen(false)
      setDirectoryPath('')
    } catch (error) {
      setConnectError(error instanceof Error ? error.message : 'Could not connect that directory.')
    } finally {
      setConnecting(false)
    }
  }
  const removeSource = async (source: Source) => {
    if (!window.confirm(`Disconnect “${source.name}”? Grimoire will not delete files from your computer.`)) return
    const response = await apiFetch(`/api/sources/${encodeURIComponent(source.id)}`, { method: 'DELETE' })
    if (!response.ok) return
    if (reading?.source_id === source.id) setReading(null)
    if (video?.source_id === source.id) setVideo(null)
    if (playback?.tracks[playback.index].source_id === source.id) setPlayback(null)
    setSources(current => current.filter(item => item.id !== source.id))
    if (selectedSource?.id === source.id) setSelectedSource(null)
  }
  const nav = [
    { label: 'For you' as PageName, icon: Sparkles }, { label: 'Discover' as PageName, icon: Compass }, { label: 'My library' as PageName, icon: Library },
  ]

  if (linkLoading || linkError) return <section className="watch-page"><main className="watch-main"><button className="watch-back" onClick={closeWatch}>Back to library</button><p role={linkError ? 'alert' : 'status'}>{linkError || 'Opening media link…'}</p></main></section>
  if (reading) return <BookReader key={`${reading.source_id}:${reading.relative_path}`} sourceId={reading.source_id} sourceName={reading.source_name} file={reading} onClose={closeWatch}/>
  if (linkedSeries) return <section className="series-page"><header className="watch-header"><button className="watch-back" onClick={closeWatch}>Back to library</button><h1>{linkedSeries.label ?? 'Series'}</h1></header>{actionError && <p role="alert">{actionError}</p>}<MediaCollection items={linkedSeries.members} groupBy="none" busy={actionBusy} onOpen={(item, queue) => { void openMedia(item, queue) }} onSave={item => { void toggleSaved(item) }} onMetadata={setMetadataItem} onGroupMetadata={setGroupItem}/>{metadataItem && <MetadataEditor item={metadataItem} onClose={() => setMetadataItem(null)} onUpdated={() => setSourceRefresh(value => value + 1)}/>}</section>
  if (video) return <VideoPlayer key={`${video.source_id}:${video.relative_path}`} sourceId={video.source_id} sourceName={video.source_name} file={video} onClose={closeWatch}/>
  return <div className="app-shell">
    <aside className={`sidebar ${menuOpen ? 'open' : ''}`}>
      <a className="brand" href="#for-you" onClick={event => { event.preventDefault(); navigate('For you') }}><img className="brand-logo" src="/logo.svg" alt="" aria-hidden="true" width="38" height="38" /><span>grimoire<span className="brand-dot">.</span></span></a>
      <div className="profile"><div className="avatar">{user.username[0].toUpperCase()}</div><div className="profile-copy"><strong>{user.username}</strong><span>{user.is_admin ? 'Administrator' : 'Personal account'}</span></div><ChevronDown size={15} /></div>
      <div className="side-label">YOUR SPACE</div>
      <nav className="main-nav">{nav.map(({ label, icon: Icon }) => <button key={label} className={`nav-item ${!selectedSource && page === label ? 'selected' : ''}`} onClick={() => { navigate(label) }}><Icon size={18} strokeWidth={1.8} />{label}</button>)}</nav>
      <div className="side-label sources-label">YOUR SOURCES <button title="Refresh sources" onClick={() => setSourceRefresh(value => value + 1)}><RefreshCw size={14}/></button>{user.is_admin && <button title="Connect a directory" onClick={() => setConnectOpen(true)}><Plus size={15} /></button>}</div>
      {sources.map(source => <button key={source.id} className={`source-row ${selectedSource?.id === source.id ? 'source-selected' : ''}`} onClick={() => { setSelectedSource(source); setGenre(''); setFilter('All'); setQuery(''); setMenuOpen(false); setActionError('') }}><span className="source-icon owner"><FolderOpen size={16} /></span><span className="source-copy"><strong>{source.name}</strong><small>{source.is_owner ? 'Owned source' : 'Shared with you'}</small></span><span className={`source-dot ${source.available ? 'online' : ''}`} /></button>)}
      {sourcesError && <p className="source-error no-sources" role="alert">{sourcesError}</p>}
      {sources.length === 0 && <p className="no-sources">No sources available yet.</p>}
      {user.is_admin && <button className="add-source" onClick={() => setConnectOpen(true)}><Plus size={15} /> Connect a directory</button>}
      <div className="sidebar-bottom"><button className="nav-item" onClick={() => setSettingsOpen(true)}><Settings2 size={18} />Account settings</button><button className="nav-item" onClick={logout}>Sign out</button><div className="connection"><span className={`connection-dot ${apiStatus}`} />{apiStatus === 'connected' ? 'Connected to Grimoire' : apiStatus === 'offline' ? 'API not connected' : 'Connecting…'}</div></div>
    </aside>

    <main className="main-content" id="home">
      <header className="topbar"><button className="mobile-menu" aria-label="Toggle menu" onClick={() => setMenuOpen(!menuOpen)}><Menu size={21} /></button><div className="breadcrumbs"><span>My space</span><span className="crumb-slash">/</span><strong>{selectedSource?.name ?? page}</strong></div><label className="search-box"><Search size={17} /><input ref={search} aria-label="Search your media" value={query} onChange={e => setQuery(e.target.value)} placeholder="Search anything..." /><kbd>Ctrl K</kbd></label><span className="top-avatar" title={user.username}>{user.username[0].toUpperCase()}</span></header>
      <div className="page-wrap">
        {actionError && <p className="catalog-notice error" role="alert">{actionError}</p>}
        {selectedSource && <section className="welcome-row"><div><div className="eyebrow"><span className="eyebrow-line" />YOUR PRIVATE MEDIA SPACE</div><h1>{selectedSource ? selectedSource.name : <>Your worlds,<br className="mobile-break" /> all in one place</>}<span className="title-period">.</span></h1><p className="welcome-copy">{selectedSource ? (selectedSource.can_access_media ? 'Files found in this connected directory.' : 'This source is visible to your account.') : 'A little something to watch, hear, or get lost in.'}</p></div>{user.is_admin && <button className="invite-button" onClick={() => setConnectOpen(true)}><Plus size={16} /> Connect directory</button>}</section>}

        {selectedSource ? <section className="directory-view">
          <div className="section-heading"><div><div className="section-kicker">LOCAL CONTENT SOURCE</div><h2>{selectedSource.can_access_media ? 'Available media' : 'Source access'}</h2></div><div className="source-heading-actions"><span>{selectedSource.can_access_media ? `${sourceFiles.length} ${sourceFiles.length === 1 ? 'file' : 'files'}` : 'Visibility only'}</span>{selectedSource.is_owner && <><button className="disconnect-button" onClick={() => setPermissionsSource(selectedSource)}><ShieldCheck size={14}/> Permissions</button><button className="disconnect-button" onClick={() => removeSource(selectedSource)} title="Disconnect this source"><Trash2 size={14}/> Disconnect</button></>}</div></div>
          <p className="source-footnote">Acquisition policy: {policyLabel(selectedSource.acquisition_policy)}. Acquisition requests are not available yet.</p>
          {!selectedSource.can_access_media ? <div className="source-empty"><ShieldCheck size={24}/><strong>Source visibility only</strong><span>The owner has not granted access to this source's files. Visibility and acquisition permission do not grant playback or reading access.</span></div> : <>
          <div className="catalog-controls"><div className="filter-row">{(['All', 'Video', 'Music', 'Books'] as MediaKind[]).map(kind => <button key={kind} onClick={() => setFilter(kind)} className={`filter-chip ${filter === kind ? 'active' : ''}`}>{kind === 'Video' && <Film size={14}/ >}{kind === 'Music' && <Headphones size={14}/ >}{kind === 'Books' && <BookOpen size={14}/ >}{kind}</button>)}</div><MetadataFilters items={sourceFiles} genre={genre} groupBy={groupBy} onGenre={setGenre} onGroup={setGroupBy}/></div>{selectedSource.is_owner && <ArrConnections key={`arr-${selectedSource.id}`} sourceId={selectedSource.id}/>}
          {selectedSource.is_owner && <SourceMetadataTools key={selectedSource.id} sourceId={selectedSource.id} onUpdated={() => setSourceRefresh(value => value + 1)}/>}
          {sourceLoading && <div className="source-message">Scanning this directory…</div>}
          {sourceError && <div className="source-message source-error">{sourceError}</div>}
          {!sourceLoading && !sourceError && filteredFiles.length === 0 && <div className="source-empty"><FolderOpen size={23}/><strong>{query || kind || genre ? 'No matching media' : 'No supported media found'}</strong><span>{query || kind || genre ? 'Try another search or filter.' : 'We look for common video, audio, EPUB, and CBZ files in this directory and its subfolders.'}</span></div>}
          {!sourceLoading && !sourceError && filteredFiles.length > 0 && <MediaCollection compact onGroupMetadata={setGroupItem} groupBy={groupBy} onMetadata={setMetadataItem} items={filteredFiles} busy={actionBusy} onOpen={(item, queue) => { void openMedia(item, queue) }} onSave={item => { void toggleSaved(item) }}/>}
          <p className="source-footnote">Select Play for music, Watch for video, or Read for EPUB and CBZ books.</p></>}
        </section> : <CatalogViews continuing={continuing} onGroupMetadata={setGroupItem} genre={genre} groupBy={groupBy} onGenre={setGenre} onGroup={setGroupBy} onMetadata={setMetadataItem} page={page} username={user.username} catalog={catalog} saved={saved} recent={recent} loading={pageLoading} error={pageError} note={pageNote} query={query} kind={kind} source={sourceFilter} sort={sort} sources={sources} busy={actionBusy} hasMore={hasMore} onKind={setKind} onSource={setSourceFilter} onSort={setSort} onRefresh={() => setSourceRefresh(value => value + 1)} onLoadMore={() => { void loadMore() }} onOpen={(item, queue) => { void openMedia(item, queue) }} onSave={item => { void toggleSaved(item) }} onDiscover={() => navigate('Discover')}/>}
        <MetadataCredits/><footer className="footer"><span>MADE FOR THE MOMENTS BETWEEN</span><span>GRIMOIRE <b>·</b> EARLY ACCESS</span></footer>
      </div>
    </main>
    {permissionsSource && <SourcePermissions source={permissionsSource} ownerUsername={user.username} onClose={() => setPermissionsSource(null)} onSaved={() => setSourceRefresh(value => value + 1)}/>}
    {settingsOpen && <AccountSettings onClose={() => setSettingsOpen(false)} />}
    {groupItem && <GroupEditor key={groupItem.metadata?.group?.id} item={groupItem} onClose={() => setGroupItem(null)} onUpdated={() => setSourceRefresh(value => value + 1)}/>}
    {metadataItem && <MetadataEditor key={metadataItem.file_key} item={metadataItem} onClose={() => setMetadataItem(null)} onUpdated={() => setSourceRefresh(value => value + 1)}/>}
    {playback && <MusicPlayer tracks={playback.tracks} index={playback.index} busy={actionBusy} onIndex={index => { void openMedia(playback.tracks[index], playback.tracks) }} onClose={() => setPlayback(null)}/>}
    {connectOpen && user.is_admin && <ConnectDirectory path={directoryPath} error={connectError} busy={connecting} onPath={setDirectoryPath} onClose={() => setConnectOpen(false)} onSubmit={connectDirectory}/>}

  </div>
}

export default function App() {
  return <Authentication>{(user, logout) => <Workspace key={user.id} user={user} logout={logout}/>}</Authentication>
}
