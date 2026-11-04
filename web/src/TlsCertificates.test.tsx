// @vitest-environment jsdom
import {act} from 'react'
import {createRoot} from 'react-dom/client'
import {expect,it,vi} from 'vitest'
import TlsCertificates from './TlsCertificates'
it('keeps automation changes as a draft until applied and preserves errors and certificate status',async()=>{
 vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT',true)
 const fetch=vi.fn(async(_url:string,init?:RequestInit)=>new Response(JSON.stringify(init?.method==='PUT'?{error:'Existing certificate required; HTTPS was left unchanged.'}:{managed:true,online:true,revision:3,settings:{primary_domain:'media.example.com',additional_domains:[],automatic:true},certificates:[{domain:'media.example.com',usable:true,expires_at:'2027-01-01T00:00:00Z',last_successful_renewal:null,error:null}]}),{status:init?.method==='PUT'?400:200}))
 vi.stubGlobal('fetch',fetch)
 const host=document.createElement('div'),root=createRoot(host)
 try{
  await act(async()=>root.render(<TlsCertificates/>))
  expect(host.textContent).toContain('Valid / covered');expect(host.textContent).toContain('No recorded renewal')
  const toggle=host.querySelector<HTMLInputElement>('input[type=checkbox]')!
  await act(async()=>toggle.click())
  expect(fetch).toHaveBeenCalledOnce()
  expect(host.textContent).toContain('HTTPS stays enabled')
  const button=[...host.querySelectorAll('button')].find(button=>button.textContent==='Apply TLS settings')!
  await act(async()=>button.click())
  expect(JSON.parse(String(fetch.mock.calls[1][1]?.body))).toMatchObject({expected_revision:3,settings:{primary_domain:'media.example.com',automatic:false}})
  expect(host.querySelector('[role="alert"]')?.textContent).toContain('HTTPS was left unchanged')
  expect(host.querySelector<HTMLInputElement>('[aria-label="Primary TLS domain"]')!.value).toBe('media.example.com')
 }finally{await act(async()=>root.unmount());vi.unstubAllGlobals()}
})
