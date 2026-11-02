import AccountSettings from './AccountSettings'
import Authentication, { type User } from './Authentication'
import { apiFetch } from './api'
import BookReader from './BookReader'
import VideoPlayer from './VideoPlayer'
import MusicPlayer from './MusicPlayer'
import { useEffect, useState, type FormEvent } from 'react'
import { AudioLines, BookOpen, Check, ChevronDown, Compass, FileAudio, FileText, Film, FolderOpen, Headphones, Library, Menu, Plus, Search, Settings2, Sparkles, Trash2, X } from 'lucide-react'

type MediaKind = 'All' | 'Video' | 'Music' | 'Books'
type MediaItem = { title: string; subtitle: string; kind: Exclude<MediaKind, 'All'>; image: string; badge: string; progress?: number }
type Source = { id: string; name: string; available: boolean }
type SourceFile = { name: string; relative_path: string; kind: 'video' | 'music' | 'book'; extension: string; size_bytes: number }

const items: MediaItem[] = [
  { title: 'The Apothecary Diaries', subtitle: 'Season 2 · Episode 08', kind: 'Video', image: 'photo-1578632767115-351597cf2477', badge: 'CONTINUE', progress: 68 },
  { title: 'A Silent Voice', subtitle: 'Film · 2h 10m', kind: 'Video', image: 'photo-1519608487953-e999c86e7455', badge: 'IN YOUR LIBRARY' },
  { title: 'Blue Hour', subtitle: 'Kacey Musgraves · 12 tracks', kind: 'Music', image: 'photo-1470252649378-9c29740c9fa8', badge: 'RECENTLY ADDED' },
  { title: 'The Left Hand of Darkness', subtitle: 'Ursula K. Le Guin · 304 pages', kind: 'Books', image: 'photo-1519682337058-a94d519337bc', badge: 'PICK UP READING', progress: 34 },
  { title: 'Mushishi', subtitle: 'Season 1 · 26 episodes', kind: 'Video', image: 'photo-1470252649378-9c29740c9fa8', badge: 'IN YOUR LIBRARY' },
  { title: 'Kind of Blue', subtitle: 'Miles Davis · 5 tracks', kind: 'Music', image: 'photo-1511379938547-c1f69419868d', badge: 'IN YOUR LIBRARY' },
]

const art = (id: string, width = 800) => `https://images.unsplash.com/${id}?auto=format&fit=crop&w=${width}&q=85`

function Workspace({ user, logout }: { user: User; logout: () => void }) {
  const [settingsOpen, setSettingsOpen] = useState(false)
  const [reading, setReading] = useState<{ source: Source; file: SourceFile } | null>(null)
  const [video, setVideo] = useState<{ source: Source; file: SourceFile } | null>(null)
  const [playback, setPlayback] = useState<{ source: Source; tracks: SourceFile[]; index: number } | null>(null)
  const [filter, setFilter] = useState<MediaKind>('All')
  const [query, setQuery] = useState('')
  const [activeNav, setActiveNav] = useState('For you')
  const [menuOpen, setMenuOpen] = useState(false)
  const [apiStatus, setApiStatus] = useState<'checking' | 'connected' | 'offline'>('checking')
  const [sources, setSources] = useState<Source[]>([])
  const [selectedSource, setSelectedSource] = useState<Source | null>(null)
  const [sourceFiles, setSourceFiles] = useState<SourceFile[]>([])
  const [sourceLoading, setSourceLoading] = useState(false)
  const [sourceError, setSourceError] = useState('')
  const [connectOpen, setConnectOpen] = useState(false)
  const [directoryPath, setDirectoryPath] = useState('')
  const [connectError, setConnectError] = useState('')
  const [connecting, setConnecting] = useState(false)

  useEffect(() => {
    apiFetch('/api/health').then(async response => {
      if (!response.ok) throw new Error('API unavailable')
      setApiStatus('connected')
      const sourceResponse = await apiFetch('/api/sources')
      if (sourceResponse.ok) setSources(await sourceResponse.json() as Source[])
    }).catch(() => setApiStatus('offline'))
  }, [])

  useEffect(() => {
    if (!selectedSource) return
    setSourceFiles([])
    setSourceLoading(true)
    setSourceError('')
    apiFetch(`/api/sources/${encodeURIComponent(selectedSource.id)}/media`)
      .then(async response => {
        const data = await response.json()
        if (!response.ok) throw new Error(data.error ?? 'Could not scan this directory.')
        setSourceFiles(data as SourceFile[])
      })
      .catch(error => setSourceError(error instanceof Error ? error.message : 'Could not scan this directory.'))
      .finally(() => setSourceLoading(false))
  }, [selectedSource])

  const filteredItems = items.filter(item => (filter === 'All' || item.kind === filter) && `${item.title} ${item.subtitle}`.toLowerCase().includes(query.toLowerCase()))
  const filteredFiles = sourceFiles.filter(file => (filter === 'All' || (filter === 'Video' && file.kind === 'video') || (filter === 'Music' && file.kind === 'music') || (filter === 'Books' && file.kind === 'book')) && `${file.name} ${file.relative_path}`.toLowerCase().includes(query.toLowerCase()))
  const formatSize = (bytes: number) => bytes < 1024 ** 2 ? `${Math.max(1, Math.round(bytes / 1024))} KB` : `${(bytes / 1024 ** 2).toFixed(1)} MB`
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
    if (reading?.source.id === source.id) setReading(null)
    if (video?.source.id === source.id) setVideo(null)
    if (playback?.source.id === source.id) setPlayback(null)
    setSources(current => current.filter(item => item.id !== source.id))
    if (selectedSource?.id === source.id) setSelectedSource(null)
  }
  const nav = [
    { label: 'For you', icon: Sparkles }, { label: 'Discover', icon: Compass }, { label: 'My library', icon: Library },
  ]

  return <div className="app-shell">
    <aside className={`sidebar ${menuOpen ? 'open' : ''}`}>
      <a className="brand" href="#home"><img className="brand-logo" src="/logo.svg" alt="" aria-hidden="true" width="38" height="38" /><span>grimoire<span className="brand-dot">.</span></span></a>
      <div className="profile"><div className="avatar">{user.username[0].toUpperCase()}</div><div className="profile-copy"><strong>{user.username}</strong><span>{user.is_admin ? 'Administrator' : 'Personal account'}</span></div><ChevronDown size={15} /></div>
      <div className="side-label">YOUR SPACE</div>
      <nav className="main-nav">{nav.map(({ label, icon: Icon }) => <button key={label} className={`nav-item ${activeNav === label ? 'selected' : ''}`} onClick={() => { setActiveNav(label); setSelectedSource(null); setMenuOpen(false) }}><Icon size={18} strokeWidth={1.8} />{label}</button>)}</nav>
      <div className="side-label sources-label">YOUR SOURCES {user.is_admin && <button title="Connect a directory" onClick={() => setConnectOpen(true)}><Plus size={15} /></button>}</div>
      {sources.map(source => <button key={source.id} className={`source-row ${selectedSource?.id === source.id ? 'source-selected' : ''}`} onClick={() => { setSelectedSource(source); setActiveNav(source.name); setFilter('All'); setMenuOpen(false) }}><span className="source-icon owner"><FolderOpen size={16} /></span><span className="source-copy"><strong>{source.name}</strong><small>Local directory</small></span><span className={`source-dot ${source.available ? 'online' : ''}`} /></button>)}
      {sources.length === 0 && <p className="no-sources">No directories connected yet.</p>}
      {user.is_admin && <button className="add-source" onClick={() => setConnectOpen(true)}><Plus size={15} /> Connect a directory</button>}
      <div className="sidebar-bottom"><button className="nav-item" onClick={() => setSettingsOpen(true)}><Settings2 size={18} />Account settings</button><button className="nav-item" onClick={logout}>Sign out</button><div className="connection"><span className={`connection-dot ${apiStatus}`} />{apiStatus === 'connected' ? 'Connected to Grimoire' : apiStatus === 'offline' ? 'API not connected' : 'Connecting…'}</div></div>
    </aside>

    <main className="main-content" id="home">
      <header className="topbar"><button className="mobile-menu" aria-label="Toggle menu" onClick={() => setMenuOpen(!menuOpen)}><Menu size={21} /></button><div className="breadcrumbs"><span>My space</span><span className="crumb-slash">/</span><strong>{activeNav}</strong></div><label className="search-box"><Search size={17} /><input aria-label="Search your media" value={query} onChange={e => setQuery(e.target.value)} placeholder="Search anything..." /><kbd>⌘ K</kbd></label><span className="top-avatar" title={user.username}>{user.username[0].toUpperCase()}</span></header>
      <div className="page-wrap">
        <section className="welcome-row"><div><div className="eyebrow"><span className="eyebrow-line" />YOUR PRIVATE MEDIA SPACE</div><h1>{selectedSource ? selectedSource.name : <>Your worlds,<br className="mobile-break" /> all in one place</>}<span className="title-period">.</span></h1><p className="welcome-copy">{selectedSource ? 'Files found in this connected directory.' : 'A little something to watch, hear, or get lost in.'}</p></div>{user.is_admin && <button className="invite-button" onClick={() => setConnectOpen(true)}><Plus size={16} /> Connect directory</button>}</section>

        {selectedSource ? <section className="directory-view">
          <div className="section-heading"><div><div className="section-kicker">LOCAL CONTENT SOURCE</div><h2>Available media</h2></div><div className="source-heading-actions"><span>{sourceFiles.length} {sourceFiles.length === 1 ? 'file' : 'files'}</span><button className="disconnect-button" onClick={() => removeSource(selectedSource)} title="Disconnect this source"><Trash2 size={14}/> Disconnect</button></div></div>
          <div className="filter-row">{(['All', 'Video', 'Music', 'Books'] as MediaKind[]).map(kind => <button key={kind} onClick={() => setFilter(kind)} className={`filter-chip ${filter === kind ? 'active' : ''}`}>{kind === 'Video' && <Film size={14}/ >}{kind === 'Music' && <Headphones size={14}/ >}{kind === 'Books' && <BookOpen size={14}/ >}{kind}</button>)}</div>
          {sourceLoading && <div className="source-message">Scanning this directory…</div>}
          {sourceError && <div className="source-message source-error">{sourceError}</div>}
          {!sourceLoading && !sourceError && filteredFiles.length === 0 && <div className="source-empty"><FolderOpen size={23}/><strong>No supported media found</strong><span>We look for common video, audio, EPUB, and CBZ files in this directory and its subfolders.</span></div>}
          {!sourceLoading && !sourceError && filteredFiles.length > 0 && <div className="file-list">{filteredFiles.map(file => <article className="file-row" key={file.relative_path}><span className={`file-kind ${file.kind}`}>{file.kind === 'video' ? <Film size={16}/> : file.kind === 'music' ? <FileAudio size={16}/> : <FileText size={16}/>}</span><div className="file-copy"><strong>{file.name}</strong><span>{file.relative_path}</span></div><span className="file-type">{file.extension.toUpperCase()}</span><span className="file-size">{formatSize(file.size_bytes)}</span>{file.kind === 'music' && <button className="track-play" aria-label={`Play ${file.name}`} onClick={() => { setVideo(null); const tracks = filteredFiles.filter(track => track.kind === 'music'); setPlayback({ source: selectedSource, tracks, index: tracks.indexOf(file) }) }}>▶ Play</button>}{file.kind === 'video' && <button className="track-play" aria-label={`Watch ${file.name}`} onClick={() => { setPlayback(null); setVideo({ source: selectedSource, file }) }}>▶ Watch</button>}{file.kind === 'book' && <button className="track-play" aria-label={`Read ${file.name}`} onClick={() => { setPlayback(null); setVideo(null); setReading({ source: selectedSource, file }) }}>Read</button>}</article>)}</div>}
          <p className="source-footnote">Select Play for music, Watch for video, or Read for EPUB and CBZ books.</p>
        </section> : <>

        <p className="sample-notice">Sample catalog preview. Select a connected source to browse your media.</p>
        <section className="featured" aria-label="Featured media">
          <img className="featured-art" src={art('photo-1470252649378-9c29740c9fa8', 1800)} alt="Sunrise over a quiet mountain landscape" />
          <div className="featured-shade" />
          <div className="featured-content"><span className="featured-label"><Sparkles size={13} /> A MOONLIGHT ARCHIVE PICK</span><h2>Find your<br />quiet place.</h2><p>Some stories don't need to be loud<br className="desktop-only" /> to stay with you.</p><button onClick={() => setFilter('Video')} className="featured-cta">Explore the collection <span>↗</span></button></div>
          <div className="featured-credit">CURATED BY MOONLIGHT ARCHIVE <span>·</span> 18 TITLES</div>
        </section>

        <section className="continue-section"><div className="section-heading"><div><div className="section-kicker">PICK UP WHERE YOU LEFT OFF</div><h2>Still on your mind</h2></div><button className="text-link" onClick={() => setFilter('All')}>View history <span>↗</span></button></div>
          <div className="continue-card"><div className="continue-poster"><img src={art(items[0].image)} alt="Atmospheric anime artwork"/><span className="play-icon">▶</span></div><div className="continue-info"><div className="continue-type"><Film size={13}/> VIDEO <span>·</span> MOONLIGHT ARCHIVE</div><h3>The Apothecary Diaries</h3><p>Season 2 <span>·</span> Episode 08, “The Dancing Ghost”</p><div className="progress-track"><span style={{width:'68%'}}/></div><div className="continue-foot"><span>18 min left</span><button onClick={() => setActiveNav('Continue watching')}>Continue watching <span>→</span></button></div></div><button className="more-button" aria-label="More options">···</button></div>
        </section>

        <section className="library-section"><div className="section-heading library-heading"><div><div className="section-kicker">A FEW THINGS YOU LOVE</div><h2>From your library</h2></div><button className="text-link" onClick={() => setActiveNav('My library')}>Open library <span>↗</span></button></div>
          <div className="filter-row">{(['All', 'Video', 'Music', 'Books'] as MediaKind[]).map(kind => <button key={kind} onClick={() => setFilter(kind)} className={`filter-chip ${filter === kind ? 'active' : ''}`}>{kind === 'Video' && <Film size={14}/ >}{kind === 'Music' && <Headphones size={14}/ >}{kind === 'Books' && <BookOpen size={14}/ >}{kind}</button>)}</div>
          <div className="media-grid">{filteredItems.map((item, index) => <article className="media-card" key={item.title} style={{animationDelay:`${index * 45}ms`}}><div className="media-art"><img src={art(item.image)} alt="" loading="lazy"/><span className="media-badge">{item.badge}</span><button className="card-play" aria-label={`Play ${item.title}`}>{item.kind === 'Music' ? <AudioLines size={18}/> : item.kind === 'Books' ? <BookOpen size={18}/> : <span>▶</span>}</button>{item.progress && <div className="card-progress"><span style={{width:`${item.progress}%`}}/></div>}</div><div className="media-meta"><span className="kind-label">{item.kind === 'Video' ? '◉' : item.kind === 'Music' ? '♫' : '▤'} &nbsp;{item.kind.toUpperCase()}</span><span className="media-ready"><Check size={11}/> READY</span></div><h3>{item.title}</h3><p>{item.subtitle}</p></article>)}</div>
          {filteredItems.length === 0 && <div className="empty-state">Nothing here yet. Try a different search or media type.</div>}
          <button className="browse-button" onClick={() => { setFilter('All'); setQuery(''); setActiveNav('Discover') }}><Compass size={15}/> Browse your whole collection</button>
        </section>
        </>}
        <footer className="footer"><span>MADE FOR THE MOMENTS BETWEEN</span><span>GRIMOIRE <b>·</b> EARLY ACCESS</span></footer>
      </div>
    </main>
    {settingsOpen && <AccountSettings onClose={() => setSettingsOpen(false)} />}
    {reading && <BookReader sourceId={reading.source.id} sourceName={reading.source.name} file={reading.file} onClose={() => setReading(null)} />}
    {video && <VideoPlayer sourceId={video.source.id} sourceName={video.source.name} file={video.file} onClose={() => setVideo(null)} />}
    {playback && <MusicPlayer sourceName={playback.source.name} sourceId={playback.source.id} tracks={playback.tracks} index={playback.index} onIndex={index => setPlayback(current => current ? { ...current, index } : null)} onClose={() => setPlayback(null)} />}
    {connectOpen && user.is_admin && <div className="modal-backdrop" onMouseDown={event => { if (event.target === event.currentTarget) setConnectOpen(false) }}><form className="connect-modal" onSubmit={connectDirectory}><button className="modal-close" type="button" aria-label="Close" onClick={() => setConnectOpen(false)}><X size={17}/></button><span className="modal-icon"><FolderOpen size={19}/></span><div className="section-kicker">ADD A CONTENT SOURCE</div><h2>Connect a directory</h2><p>Choose an existing folder on the computer running Grimoire. We’ll scan its subfolders for supported media.</p><label htmlFor="directory-path">Directory path</label><input id="directory-path" autoFocus value={directoryPath} onChange={event => setDirectoryPath(event.target.value)} placeholder="/home/you/Media" required/><small className="path-hint">Use a full local path, such as <code>/home/you/Videos</code>.</small>{connectError && <div className="connect-error">{connectError}</div>}<button className="connect-submit" type="submit" disabled={connecting}>{connecting ? 'Connecting…' : 'Connect directory'}</button><span className="modal-note">Disconnecting a source never deletes its files.</span></form></div>}
  </div>
}

export default function App() {
  return <Authentication>{(user, logout) => <Workspace key={user.id} user={user} logout={logout}/>}</Authentication>
}
