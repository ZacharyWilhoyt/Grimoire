import {useState} from 'react'
import {RefreshCw} from 'lucide-react'
import {apiFetch} from './api'
import {displayTitle, type MediaItem} from './media'
import SourceMetadataTools from './SourceMetadataTools'

type Source = {id:string;name:string;remote?:boolean}
type Props = {
  sources:Source[]; items:MediaItem[]; query:string; loading:boolean; error:string
  onRefresh:()=>void; onMetadata:(item:MediaItem)=>void; onGroup:(item:MediaItem)=>void
}

export default function MetadataPage(p:Props) {
  const [source,setSource]=useState('')
  const [selected,setSelected]=useState<string[]>([])
  const [target,setTarget]=useState('')
  const [search,setSearch]=useState('')
  const [busy,setBusy]=useState(false)
  const [notice,setNotice]=useState('')
  const [error,setError]=useState('')
  const active=p.sources.find(item=>item.id===source)??p.sources[0]
  const own=p.items.filter(item=>item.source_id===active?.id)
  const shown=own.filter(item=>`${displayTitle(item)} ${item.relative_path}`.toLowerCase().includes(p.query.trim().toLowerCase()))
  const chosen=own.filter(item=>item.location_id&&selected.includes(item.location_id))
  const groups=new Map<string,MediaItem>()
  for(const item of p.items) {
    const group=item.metadata?.group
    if(item.location_id&&group&&!item.collection_link&&!selected.includes(item.location_id)
      &&chosen.every(member=>member.kind===item.kind))groups.set(`${item.source_id}:${group.id}`,item)
  }
  const candidates=[...groups.values()].filter(item=>`${item.metadata!.group!.label} ${item.source_name}`.toLowerCase().includes(search.trim().toLowerCase()))
    .sort((a,b)=>a.metadata!.group!.label.localeCompare(b.metadata!.group!.label))
  const anchor=candidates.find(item=>item.location_id===target)

  async function update(remove=false,copy=false) {
    if(busy||!chosen.length||(!remove&&!anchor))return
    setBusy(true);setError('');setNotice('')
    try {
      const response=copy
        ? await apiFetch(`/api/sources/${encodeURIComponent(chosen[0].source_id)}/metadata/group-membership`,{
          method:'POST',headers:{'Content-Type':'application/json'},
          body:JSON.stringify({path:chosen[0].relative_path,target_path:anchor!.relative_path,expected_group_id:anchor!.metadata!.group!.id}),
        })
        : await apiFetch('/api/catalog/v1/collection-membership',{
          method:'POST',headers:{'Content-Type':'application/json'},
          body:JSON.stringify({locations:chosen.map(item=>item.location_id),
            ...(remove?{}:{target:anchor!.location_id,group:anchor!.metadata!.group!.id})}),
        })
      if(!response.ok)throw Error((await response.json()).error??'Could not update collection membership.')
      setNotice(remove?'Collection links removed.':`Added ${chosen.length} ${chosen.length===1?'file':'files'} to ${anchor!.metadata!.group!.label}.`)
      setSelected([]);setTarget('');p.onRefresh()
    } catch(error) {setError(error instanceof Error?error.message:'Could not update collection membership.')}
    finally {setBusy(false)}
  }

  return <section aria-labelledby="metadata-page-title">
    <div className="welcome-row"><div><h1 id="metadata-page-title">Metadata</h1><p className="welcome-copy">Edit your files and add them to existing collections across accessible sources.</p></div>
      <button className="invite-button" onClick={p.onRefresh} disabled={busy||p.loading}><RefreshCw size={15}/> Refresh</button></div>
    {(p.error||error)&&<p className="catalog-notice error" role="alert">{error||p.error}</p>}
    {notice&&<p className="catalog-notice" role="status">{notice}</p>}
    {p.loading&&<p role="status">Loading media…</p>}
    {!active&&!p.loading&&<p className="catalog-notice">Connect a source you own to edit and group its media.</p>}
    {active&&<>
      <div className="catalog-controls"><label>Your source <select value={active.id} disabled={busy} onChange={event=>{setSource(event.target.value);setSelected([]);setTarget('');setNotice('')}}>
        {p.sources.map(item=><option key={item.id} value={item.id}>{item.name}</option>)}
      </select></label></div>
      {!active.remote&&<SourceMetadataTools key={active.id} sourceId={active.id} onUpdated={p.onRefresh}/>}
      <div className="metadata-workspace">
        <section><h2>Files from {active.name}</h2><ul className="metadata-file-list">
          {shown.map(item=><li key={item.file_key}>
            <label><input type="checkbox" aria-label={`Select ${displayTitle(item)}`}
              checked={!!item.location_id&&selected.includes(item.location_id)}
              disabled={!item.location_id||busy||!!chosen.length&&chosen[0].kind!==item.kind||selected.length>=50&&!selected.includes(item.location_id??'')}
              onChange={event=>{const id=item.location_id!;setTarget('');setSelected(old=>event.target.checked?[...old,id]:old.filter(value=>value!==id))}}/>
              <span><strong>{displayTitle(item)}</strong><small>{item.collection_link?.label??item.metadata?.group?.label??item.extension.toUpperCase()}</small></span>
            </label>
            {!item.remote&&item.available&&<button className="theme-button" onClick={()=>p.onMetadata(item)}>Edit metadata</button>}
            {item.metadata?.group?.can_edit&&item.available&&<button className="theme-button" onClick={()=>p.onGroup(item)}>Edit group</button>}
          </li>)}
        </ul>{!shown.length&&!p.loading&&<p className="welcome-copy">No files match in this source.</p>}
        {shown.some(item=>!item.location_id)&&<p className="welcome-copy">Refresh to select files still being indexed.</p>}</section>
        <section><h2>Choose a collection</h2>
          <p className="welcome-copy">Select up to 50 files of one media type.</p>
          <label className="metadata-target-search">Find a series or album <input value={search} onChange={event=>{setSearch(event.target.value);setTarget('')}}/></label>
          <label className="metadata-target-search">Existing collection <select value={target} onChange={event=>setTarget(event.target.value)} disabled={busy||!chosen.length}>
            <option value="">Choose a collection</option>
            {candidates.map(item=><option key={item.location_id} value={item.location_id}>{item.metadata!.group!.label} · {item.source_name}</option>)}
          </select></label>
          <div className="metadata-workspace-actions"><button className="theme-button theme-button-primary" disabled={busy||!anchor||!chosen.length} onClick={()=>void update()}>Add to collection</button>
            {chosen.length===1&&!chosen[0].remote&&!chosen[0].collection_link&&anchor?.source_id===chosen[0].source_id&&<button className="theme-button" disabled={busy} onClick={()=>void update(false,true)}>Copy group metadata</button>}
            <button className="theme-button" disabled={busy||!chosen.some(item=>item.collection_link)} onClick={()=>void update(true)}>Remove collection links</button></div>
        </section>
      </div>
    </>}
  </section>
}
