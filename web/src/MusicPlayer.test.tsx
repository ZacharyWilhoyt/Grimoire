// @vitest-environment jsdom
import { act } from 'react'
import { createRoot } from 'react-dom/client'
import { expect, it, vi } from 'vitest'
import MusicPlayer from './MusicPlayer'
vi.mock('./useMediaProgress', () => ({ useMediaProgress: () => ({ media: { current: null }, progressError: '' }) }))
it('cycles repeat states, toggles shuffle, and preserves the selected queue', async () => {
  vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT',true)
  const host=document.createElement('div');document.body.append(host);const root=createRoot(host), change=vi.fn()
  const tracks=Array.from({length:3},(_,index)=>({name:`Track ${index}`,relative_path:`${index}.mp3`,source_id:'source',source_name:'Source'}))
  try {
    await act(async()=>root.render(<MusicPlayer tracks={tracks} index={0} busy={false} onIndex={change} onClose={()=>{}}/>))
    const audio=()=>host.querySelector('audio')!
    const button=(label:string)=>host.querySelector<HTMLButtonElement>(`button[aria-label="${label}"]`)!
    expect(audio().loop).toBe(false)
    await act(async()=>button('Repeat: off').click());expect(button('Repeat: all').getAttribute('aria-pressed')).toBe('true');expect(audio().loop).toBe(false)
    await act(async()=>button('Repeat: all').click());expect(audio().loop).toBe(true)
    await act(async()=>button('Repeat: one').click());expect(audio().loop).toBe(false)
    await act(async()=>button('Shuffle').click());expect(button('Shuffle').getAttribute('aria-pressed')).toBe('true')
    await act(async()=>button('Next track').click());expect([1,2]).toContain(change.mock.calls[0][0])
  } finally {await act(async()=>root.unmount());host.remove();vi.unstubAllGlobals()}
})
it('loops a one-track queue when repeat-all is selected', async()=>{
  vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT',true)
  const host=document.createElement('div'),root=createRoot(host)
  try {
    await act(async()=>root.render(<MusicPlayer tracks={[{name:'Only',relative_path:'only.mp3',source_id:'source',source_name:'Source'}]} index={0} busy={false} onIndex={()=>{}} onClose={()=>{}}/>))
    await act(async()=>host.querySelector<HTMLButtonElement>('button[aria-label="Repeat: off"]')!.click())
    expect(host.querySelector('audio')!.loop).toBe(true)
    expect(host.querySelector<HTMLButtonElement>('button[aria-label="Shuffle"]')!.disabled).toBe(true)
  } finally {await act(async()=>root.unmount());vi.unstubAllGlobals()}
})
