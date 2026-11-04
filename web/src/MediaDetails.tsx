import {ArrowLeft,BookOpen,Film} from 'lucide-react'
import {displayTitle,type MediaItem} from './media'
import MediaArtwork from './MediaArtwork'
import MetadataCredits from './MetadataCredits'
export default function MediaDetails({item,onClose,onOpen}:{item:MediaItem;onClose:()=>void;onOpen:()=>void}){
 const fields=item.metadata?.fields
 return <section className="media-details-page"><header className="watch-header"><button className="watch-back" onClick={onClose}><ArrowLeft/> Back to library</button><h1>{displayTitle(item)}</h1></header><div className="media-details-layout"><MediaArtwork item={item} portrait/><div><p className="section-kicker">{item.kind==='book'?'BOOK':'VIDEO'} · {item.source_name}</p><h2>{displayTitle(item)}</h2>{fields?.creator&&<p>{fields.creator}</p>}{fields?.collection&&<p>{fields.collection}{fields.volume!=null?` · Volume ${fields.volume}`:''}</p>}{fields?.description&&<p className="media-details-description">{fields.description}</p>}<div className="media-details-tags">{fields?.genres?.map(genre=><span key={genre}>{genre}</span>)}</div><button className="track-play" disabled={!item.available} onClick={onOpen}>{item.kind==='book'?<BookOpen size={17}/>:<Film size={17}/>} {item.kind==='book'?'Read book':'Watch video'}</button><MetadataCredits credits={item.metadata?.credits??[]}/></div></div></section>
}
