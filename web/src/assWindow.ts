// Keep ASS timestamps and event order intact. Long-running/overlapping typesetting
// stays present for its full lifetime; uncertain event formats fall back to full text.
export function assWindow(script:string){
 const lines=script.split(/\r?\n/),timings=new Map<number,{start:number;end:number}>()
 let section='',start=-1,end=-1,formatKnown=false
 const timestamp=(value:string)=>{const match=/^(\d+):(\d{2}):(\d{2}(?:\.\d+)?)$/.exec(value.trim());return match?Number(match[1])*3600+Number(match[2])*60+Number(match[3]):NaN}
 for(let index=0;index<lines.length;index++){
  const line=lines[index],heading=/^\s*\[([^\]]+)\]/.exec(line)
  if(heading){section=heading[1].toLowerCase();continue}
  if(section!=='events')continue
  if(/^\s*Format:/i.test(line)){const fields=line.slice(line.indexOf(':')+1).split(',').map(f=>f.trim().toLowerCase());start=fields.indexOf('start');end=fields.indexOf('end');formatKnown=start>=0&&end>=0;continue}
  if(!/^\s*Dialogue:/i.test(line))continue
  if(!formatKnown)return {at:(_time:number)=>script,key:(_time:number)=>0}
  const fields=line.slice(line.indexOf(':')+1).split(',')
  const from=timestamp(fields[start]??''),to=timestamp(fields[end]??'')
  if(!Number.isFinite(from)||!Number.isFinite(to)||to<from)return {at:(_time:number)=>script,key:(_time:number)=>0}
  timings.set(index,{start:from,end:to})
 }
 const key=(time:number)=>Math.max(0,Math.floor((Number.isFinite(time)?time:0)/60))
 return {key,at(time:number){const from=key(time)*60-30,to=key(time)*60+120;return lines.filter((_,index)=>{const timing=timings.get(index);return !timing||timing.end>=from&&timing.start<=to}).join('\n')}}
}
