import { expect, it } from 'vitest'
import { adjacentTrack, playOrder } from './musicQueue'
it('keeps sequential playback and end boundaries as the default', () => {
  expect(playOrder(3,1,false)).toEqual([0,1,2])
  expect(adjacentTrack([0,1,2],2,1,'off')).toBeUndefined()
  expect(adjacentTrack([0,1,2],0,-1,'off')).toBeUndefined()
  expect(adjacentTrack([0,1,2],1,1,'off')).toBe(2)
})
it('shuffles every eligible track once while keeping the current track first', () => {
  const order=playOrder(8,3,true,()=>0.4)
  expect(order[0]).toBe(3)
  expect([...order].sort()).toEqual([0,1,2,3,4,5,6,7])
  expect(new Set(order).size).toBe(8)
  expect(adjacentTrack(order,order[1],-1,'off')).toBe(3)
})
it('wraps repeat-all in either direction and keeps repeat-one for native looping', () => {
  expect(adjacentTrack([2,0,1],1,1,'all')).toBe(2)
  expect(adjacentTrack([2,0,1],2,-1,'all')).toBe(1)
  expect(adjacentTrack([0,1],1,1,'one')).toBeUndefined()
  expect(adjacentTrack([],0,1,'all')).toBeUndefined()
})
