// @vitest-environment jsdom
import {act} from 'react'
import {createRoot} from 'react-dom/client'
import {expect,it,vi} from 'vitest'
import ArrConnections from './ArrConnections'
import {apiFetch} from './api'
vi.mock('./api',()=>({apiFetch:vi.fn()}))
it('opens source settings and creates valid connection IDs without secure-context randomUUID',async()=>{
 vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT',true)
 const getRandomValues=crypto.getRandomValues.bind(crypto)
 vi.stubGlobal('crypto',{getRandomValues})
 const calls=vi.mocked(apiFetch)
 calls.mockImplementation(async(_url,options)=>options?.method==='PUT'?new Response(null,{status:204}):new Response(JSON.stringify({configured:true,online:true,connections:[]})))
 const host=document.createElement('div'),root=createRoot(host)
 try{
  await act(async()=>root.render(<ArrConnections sourceId="source"/>))
  expect(host.textContent).toContain('Optional Arr connections')
  await act(async()=>host.querySelector<HTMLButtonElement>('button')!.click())
  expect(host.textContent).toContain('Add a connection')
  await act(async()=>host.querySelector('form')!.dispatchEvent(new Event('submit',{bubbles:true,cancelable:true})))
  const write=calls.mock.calls.find(([,options])=>options?.method==='PUT')
  expect(write?.[0]).toMatch(/^\/api\/sources\/source\/arr\/[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/)
 }finally{await act(async()=>root.unmount());vi.unstubAllGlobals();calls.mockReset()}
})
