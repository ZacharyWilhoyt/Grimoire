import {expect,it} from 'vitest'
import {albumKey,artistKey,musicGroups,artistName,songsFor,musicDuration} from './musicLibrary'
import type {MediaItem} from './media'
const song=(key:string,artist:string,album:string,source='one')=>({file_key:key,kind:'music',source_id:source,metadata:{fields:{creator:artist,collection:album,title:key,track:Number(key)}}} as MediaItem)
it('intersects artist and album filters and keeps albums on different sources separate',()=>{
 const first=song('1','Artist','Album'),second=song('2','Artist','Other'),third=song('3','Other artist','Album'),copy=song('4','Artist','Album','two')
 const items=[first,second,third,copy]
 expect(songsFor(items,artistKey(first),albumKey(first),'','track')).toEqual([first])
 expect(songsFor(items,artistKey(third),albumKey(first),'','track')).toEqual([])
 expect(albumKey(first)).not.toBe(albumKey(copy))
 expect(musicGroups(items,artistKey,artistName)).toHaveLength(2)
 expect(musicDuration(null)).toBe('—')
 expect(musicDuration(185)).toBe('3:05')
})
