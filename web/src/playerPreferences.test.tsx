// @vitest-environment jsdom
import {act} from 'react'
import {createRoot} from 'react-dom/client'
import {expect,it,vi} from 'vitest'
import {PlayerPreferences,usePlayerPreferences} from './playerPreferences'
it('restores user preferences across mounts without sharing them between accounts',async()=>{
 vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT',true);localStorage.clear()
 const host=document.createElement('div'),root=createRoot(host)
 function Controls(){const p=usePlayerPreferences()!;return <button onClick={()=>p.update({height:1080,subtitle:{label:'English',format:'ass'}})}>{JSON.stringify(p.settings)}</button>}
 const render=(user:string)=>root.render(<PlayerPreferences key={user} userId={user}><Controls/></PlayerPreferences>)
 try{
 await act(async()=>render('one'));await act(async()=>host.querySelector('button')!.click())
 await act(async()=>render('two'));expect(host.textContent).toContain('"height":0');expect(host.textContent).not.toContain('English')
 await act(async()=>render('one'));expect(host.textContent).toContain('"height":1080');expect(host.textContent).toContain('English')
 }finally{await act(async()=>root.unmount());localStorage.clear();vi.unstubAllGlobals()}
})
