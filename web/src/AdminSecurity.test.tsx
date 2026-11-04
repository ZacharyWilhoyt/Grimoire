// @vitest-environment jsdom
import {act} from 'react'
import {createRoot} from 'react-dom/client'
import {expect,it,vi} from 'vitest'
import AdminSecurity from './AdminSecurity'
it('requires review and lockout acknowledgement before saving IP rules with their revision',async()=>{
 vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT',true)
 Object.defineProperty(HTMLDialogElement.prototype,'showModal',{configurable:true,value:vi.fn()})
 Object.defineProperty(HTMLDialogElement.prototype,'close',{configurable:true,value:vi.fn()})
 const policy={default_action:'deny',allow:[],deny:[]}
 const fetch=vi.fn(async(url:string,init?:RequestInit)=>new Response(JSON.stringify(url.endsWith('preview')?{policy,would_block_you:true}:init?.method==='PUT'?{revision:8,policy,current_ip:'127.0.0.1'}:{revision:7,policy:{default_action:'allow',allow:[],deny:[]},current_ip:'127.0.0.1'})))
 vi.stubGlobal('fetch',fetch)
 const host=document.createElement('div'),root=createRoot(host)
 try{
  await act(async()=>root.render(<AdminSecurity onClose={()=>{}}/>))
  const button=(text:string)=>[...host.querySelectorAll('button')].find(button=>button.textContent===text)!
  expect(button('Save IP rules').disabled).toBe(true)
  await act(async()=>button('Review rules').click())
  expect(host.textContent).toContain('block your current IP')
  expect(button('Save IP rules').disabled).toBe(true)
  await act(async()=>host.querySelector<HTMLInputElement>('input[type=checkbox]')!.click())
  expect(button('Save IP rules').disabled).toBe(false)
  await act(async()=>button('Save IP rules').click())
  const save=fetch.mock.calls.find(([,init])=>init?.method==='PUT')!
  expect(JSON.parse(String(save[1]?.body))).toMatchObject({policy,expected_revision:7,acknowledge_lockout:true})
 }finally{await act(async()=>root.unmount());vi.unstubAllGlobals()}
})
