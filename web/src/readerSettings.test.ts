// @vitest-environment jsdom
import {expect,it} from 'vitest'
import {isSpread,spreadMembers,readSettings} from './readerSettings'
it('keeps landscape spreads alone and pairs only confirmed portrait pages',()=>{
 expect(isSpread(1600,1000)).toBe(true)
 expect(isSpread(800,1200)).toBe(false)
 expect(spreadMembers(0,4,{0:false,1:false},'double')).toEqual([0,1])
 expect(spreadMembers(0,4,{0:false,1:true},'double')).toEqual([0])
 expect(spreadMembers(0,4,{0:false},'double')).toEqual([0])
 expect(spreadMembers(3,4,{3:false},'double')).toEqual([3])
})
it('isolates reading profiles and validates stored values',()=>{
 localStorage.setItem('reader:Manga',JSON.stringify({zoom:900,layout:'double',fit:'height',direction:'rtl'}))
 expect(readSettings('reader','Manga')).toMatchObject({zoom:400,layout:'double',fit:'height',direction:'rtl'})
 expect(readSettings('reader','Novel')).toMatchObject({zoom:100,layout:'single'})
 localStorage.removeItem('reader:Manga')
})
