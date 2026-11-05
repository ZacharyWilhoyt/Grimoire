import {expect,it} from 'vitest'
import {continuation} from './dashboard'
import type {MediaItem} from './media'
const item=(key:string,kind:'video'|'book',position:number,time:number,collection='Series')=>({file_key:key,kind,available:true,progress:{position,total:10,updated_at:time},metadata:{fields:{collection}}} as MediaItem)
it('separates media, keeps the newest unfinished item per series, and includes the first book chapter',()=>{
 const records=[item('old','video',3,1),item('new','video',4,2),item('complete','video',10,3),item('first','book',0,2),{...item('finished','book',9,4),progress:{position:9,total:10,updated_at:4,completed:true}}]
 expect(continuation(records,'video').map(item=>item.file_key)).toEqual(['new'])
 expect(continuation(records,'book').map(item=>item.file_key)).toEqual(['first'])
})
it('offers the next available unread volume without fabricating a saved position',()=>{
 const previous={...item('one','book',9,3),progress:{position:9,total:10,updated_at:3,completed:true},metadata:{fields:{collection:'Series',volume:1}},source_id:'s'} as MediaItem
 const next={...item('two','book',0,0),progress:null,metadata:{fields:{collection:'Series',volume:2}},source_id:'s'} as MediaItem
 const result=continuation([],'book',[previous,next])
 expect(result).toEqual([next]);expect(result[0].progress).toBeNull()
})
