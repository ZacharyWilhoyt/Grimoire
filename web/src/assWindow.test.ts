import {expect,it} from 'vitest'
import {assWindow} from './assWindow'
it('preserves styles, absolute timing, ordering, and overlapping ASS effects',()=>{
 const header='[Script Info]\nPlayResX: 1920\n[V4+ Styles]\nStyle: Default,Example\n[Events]\nFormat: Layer, Start, End, Style, Text\n'
 const long='Dialogue: 4,0:00:00.00,0:20:00.00,Default,{\\move(1,2,3,4)\\t(0,1000,\\blur3)}Sign, with commas'
 const now='Dialogue: 0,0:05:01.00,0:05:04.00,Default,{\\k50}Words'
 const future='Dialogue: 0,0:19:01.00,0:19:04.00,Default,Future'
 const windows=assWindow(header+[long,now,future].join('\n'))
 expect(windows.at(301)).toBe(header+[long,now].join('\n'))
 expect(windows.at(1140)).toContain(future)
 expect(windows.at(1140)).not.toContain(now)
 expect(windows.at(1140)).toContain(long)
})
it('falls back to the complete script for unknown event formats or timing',()=>{
 const script='[Events]\nDialogue: Unknown,format'
 expect(assWindow(script).at(200)).toBe(script)
})
