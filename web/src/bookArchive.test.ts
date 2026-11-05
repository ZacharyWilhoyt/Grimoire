import { afterEach, describe, expect, it, vi } from 'vitest'
import { BlobWriter, TextReader, ZipWriter, Uint8ArrayReader } from '@zip.js/zip.js'
import { archivePath, openArchive, MAX_CBZ_BYTES } from './bookArchive'

afterEach(() => vi.unstubAllGlobals())
async function fixture() {
  const writer = new ZipWriter(new BlobWriter(), { useWebWorkers: false, level: 0 })
  await writer.add('chapter.xhtml', new TextReader('<p>Hello</p>'))
  return writer.close()
}
function mockBook(blob: Blob, prefix=0) {
  const total=prefix+blob.size
  const requests:{start:number;end:number}[]=[]
  const fetch=vi.fn(async(_url:string,init?:RequestInit)=>{
    const header=new Headers(init?.headers).get('Range')!
    const match=/bytes=(\d+)-(\d+)/.exec(header)!
    const start=Number(match[1]),end=Number(match[2]);requests.push({start,end})
    const bytes=new Uint8Array(end-start+1)
    if(end>=prefix){const offset=Math.max(start,prefix);bytes.set(new Uint8Array(await blob.slice(offset-prefix,end-prefix+1).arrayBuffer()),offset-start)}
    return new Response(bytes,{status:206,headers:{'content-range':`bytes ${start}-${end}/${total}`,'last-modified':'Mon, 01 Jan 2024 00:00:00 GMT'}})
  })
  vi.stubGlobal('fetch',fetch)
  return {fetch,requests}
}

describe('book archives', () => {
  it('resolves EPUB resources while rejecting external origins', () => {
    expect(archivePath('OPS/text/chapter.xhtml', '../images/cover.png')).toBe('OPS/images/cover.png')
    expect(() => archivePath('OPS/book.opf', 'https://example.com/book')).toThrow()
    expect(() => archivePath('OPS/book.opf', 'javascript:alert(1)')).toThrow()
  })
  it('reads archive resources and rejects missing entries', async () => {
    mockBook(await fixture())
    const book = await openArchive('/book', new AbortController().signal)
    try {
      expect(await (await book.read('chapter.xhtml')).text()).toBe('<p>Hello</p>')
      await expect(book.read('missing')).rejects.toThrow('missing')
    } finally { await book.close() }
  })
  it('rejects oversized expanded entries before extraction', async () => {
    const bytes = new Uint8Array(await (await fixture()).arrayBuffer())
    const view = new DataView(bytes.buffer)
    for (let i = 0; i < bytes.length - 46; i++) {
      if (view.getUint32(i, true) === 0x02014b50) { view.setUint32(i + 24, 33 * 1024 * 1024, true); break }
    }
    mockBook(new Blob([bytes]))
    await expect(openArchive('/book', new AbortController().signal)).rejects.toThrow('limits')
  })
  it('reports invalid archives', async () => {
    mockBook(new Blob(['not a zip file']))
    await expect(openArchive('/book', new AbortController().signal)).rejects.toThrow()
  })
})

 it('opens a CBZ above the old limit using only small ranges and reads its page lazily',async()=>{
   const bytes=new Uint8Array(await (await fixture()).arrayBuffer()),view=new DataView(bytes.buffer),prefix=129*1024**2
   for(let i=0;i<bytes.length-21;i++){
     const signature=view.getUint32(i,true)
     if(signature===0x02014b50)view.setUint32(i+42,view.getUint32(i+42,true)+prefix,true)
     if(signature===0x06054b50)view.setUint32(i+16,view.getUint32(i+16,true)+prefix,true)
   }
   const {requests,fetch}=mockBook(new Blob([bytes]),prefix)
   const book=await openArchive('/book',new AbortController().signal,'cbz')
   try{
     expect([...book.entries.keys()]).toEqual(['chapter.xhtml'])
     expect(await (await book.read('chapter.xhtml')).text()).toBe('<p>Hello</p>')
     expect(requests.reduce((sum,r)=>sum+r.end-r.start+1,0)).toBeLessThan(1024**2)
     expect(requests.every(r=>r.end-r.start+1<=512*1024)).toBe(true)
     expect(new Headers(fetch.mock.calls[1][1]?.headers).get('If-Range')).toBe('Mon, 01 Jan 2024 00:00:00 GMT')
   }finally{await book.close()}
   await expect(book.read('chapter.xhtml')).rejects.toBeDefined()
 })
 it('evicts old resources rather than retaining every extracted page, and deduplicates concurrent reads',async()=>{
   const writer=new ZipWriter(new BlobWriter(),{useWebWorkers:false})
   for(const name of ['one','two','three'])await writer.add(name,new Uint8ArrayReader(new Uint8Array(22*1024**2)))
   const {requests}=mockBook(await writer.close())
   const book=await openArchive('/book',new AbortController().signal,'cbz')
   try{
     const [first,same]=await Promise.all([book.read('one'),book.read('one')]);expect(first).toBe(same)
     await book.read('two');await book.read('three')
     const before=requests.length
     await book.read('three');expect(requests).toHaveLength(before)
     expect(await book.read('one')).not.toBe(first)
     expect(requests.length).toBeGreaterThan(before)
   }finally{await book.close()}
 },15000)
 it('refuses full-file responses rather than downloading an archive into memory',async()=>{
   const cancel=vi.fn()
   vi.stubGlobal('fetch',vi.fn(async()=>new Response(new ReadableStream({cancel}),{status:200})))
   await expect(openArchive('/book',new AbortController().signal,'cbz')).rejects.toThrow('byte ranges')
   expect(cancel).toHaveBeenCalledOnce()
 })
 it('keeps format size limits and cancels malformed oversized responses',async()=>{
   const serve=(total:number,bytes=new Uint8Array(1))=>vi.stubGlobal('fetch',vi.fn(async()=>new Response(bytes,{status:206,headers:{'content-range':`bytes 0-0/${total}`}})))
   serve(MAX_CBZ_BYTES+1)
   await expect(openArchive('/book',new AbortController().signal,'cbz')).rejects.toThrow('4 GiB')
   serve(129*1024**2)
   await expect(openArchive('/book',new AbortController().signal,'epub')).rejects.toThrow('128 MiB')
   serve(10,new Uint8Array(20))
   await expect(openArchive('/book',new AbortController().signal,'cbz')).rejects.toThrow('oversized range')
 })
