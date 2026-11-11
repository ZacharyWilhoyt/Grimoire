import {useEffect,useState,type MouseEvent} from 'react'
import {ArrowLeft,ArrowDownUp,BookOpen,Film,Headphones,Heart,Share2,Settings2,Bookmark,BookmarkCheck,Info} from 'lucide-react'
import {apiFetch} from './api'
import {displayTitle,orderMembers,progressLabel,suggestions,type MediaItem} from './media'
import {seriesLink,detailLink,type SeriesSeason,type SeriesArtwork} from './mediaLinks'
import {workKey} from './dashboard'
import MediaArtwork from './MediaArtwork'
import MediaCollection from './MediaCollection'
import MetadataCredits from './MetadataCredits'
type Personal={status:string|null;favorite:boolean;revision:number}
type Props={seriesReference?:{source:string;group:string};members:MediaItem[];label?:string|null;catalog:MediaItem[];busy:boolean;error?:string;onClose:()=>void;onOpen:(item:MediaItem,queue:MediaItem[])=>void;onDetails:(item:MediaItem)=>void;onSeries:(item:MediaItem)=>void;onSave:(item:MediaItem)=>void;onMetadata:(item:MediaItem)=>void;onGroupMetadata:(item:MediaItem)=>void;series?:boolean;seasons?:SeriesSeason[];season?:string|null;artwork?:SeriesArtwork;onSeason?:(season:string|null)=>void}
function seasonLabel(key:string){
 return key==='0'?'Specials':key==='unknown'?'Unsorted episodes':`Season ${key}`
}
export default function SeriesDetails(p:Props){
 const ordered=orderMembers(p.members),first=ordered[0],book=first?.kind==='book',music=first?.kind==='music'
 const reference=p.seriesReference
 const nativeGroup=reference?p.members.find(item=>item.source_id===reference.source&&item.metadata?.group?.id===reference.group)?.metadata?.group:first?.metadata?.group
 const group=p.series?(nativeGroup??(reference?{id:reference.group,label:p.label??'',can_edit:false,has_custom_artwork:false,has_custom_genres:false,synopsis:null,synopsis_provider:null}:null)):null
 const [tab,setTab]=useState('content'),[expanded,setExpanded]=useState(false),[sort,setSort]=useState('position'),[reverse,setReverse]=useState(false),[filter,setFilter]=useState('')
 const [personal,setPersonal]=useState<Personal|null>(null),[saving,setSaving]=useState(false),[error,setError]=useState(''),[reload,setReload]=useState(0),[share,setShare]=useState(false),[copied,setCopied]=useState(false)
 const anchor=p.seriesReference?p.members.find(item=>item.source_id===reference?.source&&item.metadata?.group?.id===reference?.group&&!item.remote):first
 const seriesSource=p.seriesReference?.source??first?.source_id,seriesGroup=p.seriesReference?.group??group?.id
 const endpoint=group&&anchor&&!anchor.remote&&seriesSource&&seriesGroup?`/api/sources/${encodeURIComponent(seriesSource)}/groups/${encodeURIComponent(seriesGroup)}/personal`:null
 const path=anchor?.relative_path
 useEffect(()=>{
  setPersonal(null);setError('');if(!endpoint||!path)return
  const abort=new AbortController()
  void apiFetch(`${endpoint}?${new URLSearchParams({path})}`,{signal:abort.signal}).then(async r=>{const data=await r.json();if(!r.ok)throw Error(data.error??'Series preferences could not be loaded.');if(!abort.signal.aborted)setPersonal(data)}).catch(e=>{if(!abort.signal.aborted)setError(e.message)})
  return()=>abort.abort()
 },[endpoint,path,reload])
 async function save(next:Personal){if(!endpoint||!anchor||saving||!personal)return;setSaving(true);setError('');try{const response=await apiFetch(endpoint,{method:'PUT',headers:{'Content-Type':'application/json'},body:JSON.stringify({path:anchor.relative_path,status:next.status,favorite:next.favorite,expected_revision:personal.revision})});const data=await response.json();if(!response.ok)throw Error(data.error??'Series preferences could not be saved.');setPersonal(data)}catch(e){setError(e instanceof Error?e.message:'Series preferences could not be saved.')}finally{setSaving(false)}}
 const [bannerFailed,setBannerFailed]=useState<string|null>(null)
 if(!first)return <section className="series-details-page"><button className="watch-back" onClick={p.onClose}><ArrowLeft/> Back to library</button><p>No accessible content is available.</p></section>
 const fields=first.metadata?.fields,title=p.series?(p.label||fields?.collection||displayTitle(first)):displayTitle(first)
 const poster=ordered.find(item=>item.available&&item.metadata?.artwork_url)??first
 const seasons=p.seasons??[]
 const seasonOverview=!!group&&first.kind==='video'&&p.season==null&&seasons.some(season=>season.key!=='unknown')
 const seriesPoster=poster.metadata?.group?.has_custom_artwork?poster.metadata.artwork_url:p.artwork?.poster_url??poster.metadata?.artwork_url
 const activePoster=seasons.find(season=>season.key===p.season)?.poster_url??seriesPoster
 const genres=[...new Set(ordered.flatMap(item=>item.metadata?.fields.genres??[]))]
 const description=group?.synopsis??ordered.find(item=>item.metadata?.fields.description)?.metadata?.fields.description
 const long=(description?.length??0)>600
 const synopsis=description&&long&&!expanded?description.slice(0,600).replace(/\s+\S*$/,'')+'…':description
 const members=(sort==='title'?[...ordered].sort((a,b)=>displayTitle(a).localeCompare(displayTitle(b),undefined,{numeric:true})):sort==='modified'?[...ordered].sort((a,b)=>(a.modified_at??0)-(b.modified_at??0)):ordered).filter(item=>`${displayTitle(item)} ${item.relative_path}`.toLowerCase().includes(filter.toLowerCase()))
 if(reverse)members.reverse()
 const available=ordered.filter(item=>item.available),resume=available.find(item=>item.progress&&!item.progress.completed&&(item.progress.total==null||item.progress.position<item.progress.total))??available.find(item=>!item.progress?.completed)??available[0]
 const currentKey=group?.id??first.file_key
 const relatedCandidates=p.catalog.filter(item=>item.available&&item.kind===first.kind&&workKey(item)!==workKey(first)&&item.metadata?.fields.genres?.some(genre=>genres.includes(genre)))
 const recommended=suggestions(relatedCandidates,new Set(p.members.map(item=>item.file_key)),18).filter((item,index,list)=>list.findIndex(other=>workKey(other)===workKey(item))===index)
 const link=window.location.origin+(group?seriesLink(seriesSource??first.source_id,seriesGroup??group.id,p.season):detailLink(first.source_id,first.relative_path))
 const volumes=[...new Set(ordered.flatMap(item=>item.metadata?.fields.volume!=null?[item.metadata.fields.volume]:[]))]
 const authors=[...new Set(ordered.flatMap(item=>item.metadata?.fields.creator?[item.metadata.fields.creator]:[]))]
 const credits=[...new Set([...ordered.flatMap(item=>item.metadata?.credits??[]),...(group?.synopsis_provider?[group.synopsis_provider]:[])])]
 function navigateSeason(event:MouseEvent<HTMLAnchorElement>,key:string|null){
  if(!p.onSeason||event.button!==0||event.ctrlKey||event.metaKey||event.altKey||event.shiftKey)return
  event.preventDefault()
  p.onSeason(key)
 }
 return <section className="series-details-page" aria-label={`${book?'Written':music?'Music':'Video'} ${p.series?'series':'title'} details`} data-series={currentKey}>
 <div className="series-backdrop" aria-hidden="true"/>
 <div className="series-details-inner"><header className="series-page-navigation"><button className="watch-back" onClick={p.onClose}><ArrowLeft/> Back to library</button>{group&&p.season!=null&&<a className="watch-back" href={seriesLink(seriesSource??first.source_id,seriesGroup??group.id)} onClick={event=>navigateSeason(event,null)}>All seasons</a>}</header>
 {p.artwork?.banner_url&&bannerFailed!==p.artwork.banner_url&&<img className="series-banner" src={p.artwork.banner_url} alt="" onError={()=>setBannerFailed(p.artwork!.banner_url)}/>}
 <div className="series-details-layout"><aside className="series-sidebar"><MediaArtwork item={poster} portrait src={activePoster}/><dl><dt>Media type</dt><dd>{book?'Written media':music?'Music':p.series?'Video series':'Video'}</dd>{fields?.year&&<><dt>{book?'Publication year':'Release year'}</dt><dd>{fields.year}</dd></>}<dt>{book?'Available files':ordered.every(item=>item.metadata?.fields.episode!=null)?'Available episodes':'Available videos'}</dt><dd>{available.length}</dd>{book&&volumes.length>0&&<><dt>Known volumes</dt><dd>{volumes.length}</dd></>}{authors.length>0&&<><dt>{book?'Authors / creators':'Creator'}</dt><dd>{authors.map(author=><span key={author} className="series-badge">{author}</span>)}</dd></>}<dt>Source</dt><dd>{first.source_name}{!first.available?' · Unavailable':''}</dd>{book&&<><dt>Formats</dt><dd>{[...new Set(ordered.map(item=>item.extension.toUpperCase()))].join(' · ')}</dd></>}</dl><MetadataCredits credits={credits}/></aside>
 <main className="series-main"><header className="series-identity"><p className="section-kicker">{book?'WRITTEN MEDIA':music?'MUSIC':'VIDEO'}{p.series?' · SERIES':''}</p><h1>{title}</h1>{p.season!=null&&<p className="series-season-heading">{seasonLabel(p.season)}</p>}<div className="series-tags">{genres.map(genre=><span className="series-badge" key={genre}>{genre}</span>)}</div></header>
 <div className="series-actions">{endpoint&&<label className="series-status">Your status<select aria-label="Your series status" value={personal?.status??''} disabled={!personal||saving} onChange={e=>{if(personal)void save({...personal,status:e.target.value||null})}}><option value="">Not set</option><option value="planning">Planning to {book?'Read':'Watch'}</option><option value="active">{book?'Reading':'Watching'}</option><option value="completed">Completed</option><option value="on_hold">On Hold</option><option value="dropped">Dropped</option></select></label>}<button className="track-play" disabled={!resume||p.busy} onClick={()=>{if(resume)p.onOpen(resume,available)}}>{book?<BookOpen size={16}/>:music?<Headphones size={16}/>:<Film size={16}/>} {resume?.progress&&!resume.progress.completed?'Continue':book?'Start reading':music?'Play':'Start watching'}</button>
 {endpoint&&<button className="series-icon-action" aria-label="Favorite series" aria-pressed={personal?.favorite??false} disabled={!personal||saving} onClick={()=>{if(personal)void save({...personal,favorite:!personal.favorite})}}><Heart size={18} fill={personal?.favorite?'currentColor':'none'}/></button>}
 <button className="series-icon-action" aria-label="Share series link" aria-expanded={share} onClick={()=>{setShare(value=>!value);if(navigator.clipboard?.writeText)void navigator.clipboard.writeText(link).then(()=>setCopied(true)).catch(()=>setCopied(false))}}><Share2 size={18}/></button>{group?.can_edit&&<button className="series-icon-action" aria-label="Edit series metadata" title="Edit series metadata" onClick={()=>p.onGroupMetadata(anchor??ordered.find(item=>item.available)??first)}><Settings2 size={18}/></button>}</div>
 {share&&<label className="series-share">{copied?'Link copied':'Copy this link'}<input aria-label="Series share link" readOnly value={link} onFocus={e=>e.currentTarget.select()}/><small>Viewers still need access to this source.</small></label>}
 {(error||p.error)&&<p className="catalog-notice error" role="alert">{error||p.error}{error&&<button className="track-play" onClick={()=>setReload(n=>n+1)}>Reload preferences</button>}</p>}
 <section className="series-synopsis"><h2>Synopsis</h2>{synopsis?<><p>{synopsis}</p>{long&&<button className="text-link" aria-expanded={expanded} onClick={()=>setExpanded(value=>!value)}>{expanded?'Show Less':'Read More'}</button>}</>:<p className="series-muted">No synopsis is available yet.</p>}</section>
 <div className="series-tabs" role="tablist" aria-label="Series content">{[['content',book?'Chapters':music?'Tracks':seasonOverview?'Seasons':'Episodes'],['relations','Relations'],['recommendations','Recommendations']].map(([key,label])=><button key={key} id={`series-tab-${key}`} role="tab" aria-controls={`series-panel-${key}`} aria-selected={tab===key} onClick={()=>setTab(key)}>{label}</button>)}</div>
 <section id="series-panel-content" role="tabpanel" aria-labelledby="series-tab-content" hidden={tab!=='content'}>{group&&p.season!=null&&<nav className="series-season-links" aria-label="Seasons">
 {seasons.map(season=><a key={season.key} aria-current={season.key===p.season?'page':undefined} href={seriesLink(seriesSource??first.source_id,seriesGroup??group.id,season.key)} onClick={event=>navigateSeason(event,season.key)}>{seasonLabel(season.key)}</a>)}
 </nav>}{seasonOverview&&group?<div className="series-season-grid">
 {seasons.map(season=><a className="series-season-card" key={season.key} href={seriesLink(seriesSource??first.source_id,seriesGroup??group.id,season.key)} onClick={event=>navigateSeason(event,season.key)}>
   <MediaArtwork item={poster} portrait src={season.poster_url??seriesPoster}/>
   <strong>{seasonLabel(season.key)}</strong>
   <small>{season.count} {season.count===1?'episode':'episodes'}</small>
 </a>)}
 </div>:<><div className="series-content-controls"><strong>{members.length} {book?'available books / volumes':'available videos'}</strong><label>Sort<select aria-label="Sort series content" value={sort} onChange={e=>setSort(e.target.value)}><option value="position">{book?'Volume / title order':'Season / episode order'}</option><option value="title">Title</option><option value="modified">Modified</option></select></label><button className="series-icon-action" aria-label={reverse?'Sort ascending':'Sort descending'} onClick={()=>setReverse(value=>!value)}><ArrowDownUp size={16}/></button><input aria-label="Filter series content" placeholder="Filter titles…" value={filter} onChange={e=>setFilter(e.target.value)}/></div>
 {book?<div className="series-chapters"><table><thead><tr><th>Volume</th><th>Title</th><th>Format</th><th>Year</th><th>Reading progress</th><th>Library</th><th>Details</th></tr></thead><tbody>{members.map(item=><tr key={item.file_key}><td>{item.metadata?.fields.volume??'—'}</td><td><button className="chapter-open" disabled={!item.available||p.busy} onClick={()=>p.onOpen(item,[item])}>{displayTitle(item)}</button></td><td>{item.extension.toUpperCase()}</td><td>{item.metadata?.fields.year??'—'}</td><td><span className={`chapter-status ${item.progress?.completed?'read':''}`}>{item.progress?.completed?'Read':item.progress?'In progress':'Unread'}</span><small>{progressLabel(item)}</small></td><td><button className="series-icon-action" aria-label={`Save ${displayTitle(item)}`} aria-pressed={item.saved_at!==null} disabled={p.busy} onClick={()=>p.onSave(item)}>{item.saved_at!==null?<BookmarkCheck size={16}/>:<Bookmark size={16}/>}</button></td><td><button className="series-icon-action" aria-label={`Metadata: ${displayTitle(item)}`} disabled={!item.available} onClick={()=>p.onMetadata(item)}><Info size={16}/></button></td></tr>)}</tbody></table>{!members.length&&<p className="dashboard-empty">No titles match this filter.</p>}</div>:<MediaCollection items={members} preserveOrder groupBy="none" busy={p.busy} onOpen={p.onOpen} onSave={p.onSave} onMetadata={p.onMetadata} onGroupMetadata={p.onGroupMetadata} onDetails={p.onDetails} onSeries={p.onSeries}/>}</>}
 </section>
 <section id="series-panel-relations" role="tabpanel" aria-labelledby="series-tab-relations" hidden={tab!=='relations'}><p className="dashboard-empty">No relationship data is indexed for this series. Adaptations, sequels, and other relationships are not inferred from similar titles.</p></section>
 <section id="series-panel-recommendations" role="tabpanel" aria-labelledby="series-tab-recommendations" hidden={tab!=='recommendations'}><p className="dashboard-subtitle">Similar genres in your accessible sources.</p>{recommended.length?<MediaCollection items={recommended} groupBy="collection" busy={p.busy} onOpen={p.onOpen} onSave={p.onSave} onMetadata={p.onMetadata} onGroupMetadata={p.onGroupMetadata} onDetails={p.onDetails} onSeries={p.onSeries}/>:<p className="dashboard-empty">No similar titles are available in the current catalog.</p>}</section>
 </main></div></div></section>
}
