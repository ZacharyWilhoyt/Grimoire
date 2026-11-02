import { useEffect, useState } from 'react'
import { AudioLines, BookOpen, Check, ChevronDown, Compass, Film, Headphones, Library, Menu, Plus, Search, Settings2, ShieldCheck, Sparkles, Tv2 } from 'lucide-react'

type MediaKind = 'All' | 'Video' | 'Music' | 'Books'
type MediaItem = { title: string; subtitle: string; kind: Exclude<MediaKind, 'All'>; image: string; badge: string; progress?: number }

const items: MediaItem[] = [
  { title: 'The Apothecary Diaries', subtitle: 'Season 2 · Episode 08', kind: 'Video', image: 'photo-1578632767115-351597cf2477', badge: 'CONTINUE', progress: 68 },
  { title: 'A Silent Voice', subtitle: 'Film · 2h 10m', kind: 'Video', image: 'photo-1519608487953-e999c86e7455', badge: 'IN YOUR LIBRARY' },
  { title: 'Blue Hour', subtitle: 'Kacey Musgraves · 12 tracks', kind: 'Music', image: 'photo-1470252649378-9c29740c9fa8', badge: 'RECENTLY ADDED' },
  { title: 'The Left Hand of Darkness', subtitle: 'Ursula K. Le Guin · 304 pages', kind: 'Books', image: 'photo-1519682337058-a94d519337bc', badge: 'PICK UP READING', progress: 34 },
  { title: 'Mushishi', subtitle: 'Season 1 · 26 episodes', kind: 'Video', image: 'photo-1470252649378-9c29740c9fa8', badge: 'IN YOUR LIBRARY' },
  { title: 'Kind of Blue', subtitle: 'Miles Davis · 5 tracks', kind: 'Music', image: 'photo-1511379938547-c1f69419868d', badge: 'IN YOUR LIBRARY' },
]

const art = (id: string, width = 800) => `https://images.unsplash.com/${id}?auto=format&fit=crop&w=${width}&q=85`

function App() {
  const [filter, setFilter] = useState<MediaKind>('All')
  const [query, setQuery] = useState('')
  const [activeNav, setActiveNav] = useState('For you')
  const [menuOpen, setMenuOpen] = useState(false)
  const [apiStatus, setApiStatus] = useState<'checking' | 'connected' | 'offline'>('checking')

  useEffect(() => {
    fetch('/api/health').then(r => r.ok ? setApiStatus('connected') : setApiStatus('offline')).catch(() => setApiStatus('offline'))
  }, [])

  const filteredItems = items.filter(item => (filter === 'All' || item.kind === filter) && `${item.title} ${item.subtitle}`.toLowerCase().includes(query.toLowerCase()))
  const nav = [
    { label: 'For you', icon: Sparkles }, { label: 'Discover', icon: Compass }, { label: 'My library', icon: Library },
  ]

  return <div className="app-shell">
    <aside className={`sidebar ${menuOpen ? 'open' : ''}`}>
      <a className="brand" href="#home"><span className="brand-mark">g</span><span>grimoire<span className="brand-dot">.</span></span></a>
      <div className="profile"><div className="avatar">M</div><div className="profile-copy"><strong>My space</strong><span>Personal library</span></div><ChevronDown size={15} /></div>
      <div className="side-label">YOUR SPACE</div>
      <nav className="main-nav">{nav.map(({ label, icon: Icon }) => <button key={label} className={`nav-item ${activeNav === label ? 'selected' : ''}`} onClick={() => { setActiveNav(label); setMenuOpen(false) }}><Icon size={18} strokeWidth={1.8} />{label}{label === 'My library' && <span className="nav-count">24</span>}</button>)}</nav>
      <div className="side-label sources-label">YOUR SOURCES <button title="Add a source"><Plus size={15} /></button></div>
      <button className="source-row"><span className="source-icon owner"><Tv2 size={16} /></span><span className="source-copy"><strong>My collection</strong><small>Personal source</small></span><span className="source-dot" /></button>
      <button className="source-row"><span className="source-icon"><ShieldCheck size={16} /></span><span className="source-copy"><strong>Moonlight Archive</strong><small>Community source</small></span><span className="source-dot online" /></button>
      <button className="add-source"><Plus size={15} /> Connect a source</button>
      <div className="sidebar-bottom"><button className="nav-item"><Settings2 size={18} />Settings</button><div className="connection"><span className={`connection-dot ${apiStatus}`} />{apiStatus === 'connected' ? 'Connected to Grimoire' : apiStatus === 'offline' ? 'API not connected' : 'Connecting…'}</div></div>
    </aside>

    <main className="main-content" id="home">
      <header className="topbar"><button className="mobile-menu" aria-label="Toggle menu" onClick={() => setMenuOpen(!menuOpen)}><Menu size={21} /></button><div className="breadcrumbs"><span>My space</span><span className="crumb-slash">/</span><strong>{activeNav}</strong></div><label className="search-box"><Search size={17} /><input aria-label="Search your media" value={query} onChange={e => setQuery(e.target.value)} placeholder="Search anything..." /><kbd>⌘ K</kbd></label><button className="top-avatar">M</button></header>
      <div className="page-wrap">
        <section className="welcome-row"><div><div className="eyebrow"><span className="eyebrow-line" />MONDAY, NOVEMBER 2, 2026</div><h1>Your worlds,<br className="mobile-break" /> all in one place<span className="title-period">.</span></h1><p className="welcome-copy">A little something to watch, hear, or get lost in.</p></div><button className="invite-button"><Plus size={16} /> Add media</button></section>

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
        <footer className="footer"><span>MADE FOR THE MOMENTS BETWEEN</span><span>GRIMOIRE <b>·</b> EARLY ACCESS</span></footer>
      </div>
    </main>
  </div>
}

export default App
