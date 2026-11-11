import { Reader, ZipReader, type FileEntry } from '@zip.js/zip.js'

export const MAX_BOOK_BYTES = 128 * 1024 * 1024
const MAX_ENTRY_BYTES = 32 * 1024 * 1024
const MAX_EXPANDED_BYTES = 256 * 1024 * 1024
export const isImage = (name: string) => /\.(jpe?g|png|webp|gif|avif)$/i.test(name)

export function archivePath(base: string, reference: string) {
  const url = new URL(reference, `https://archive.invalid/${base}`)
  if (url.origin !== 'https://archive.invalid') throw new Error('External book resources are not supported.')
  return decodeURIComponent(url.pathname.slice(1))
}

export function parseXml(text: string) {
  const document = new DOMParser().parseFromString(text, 'application/xml')
  if (document.querySelector('parsererror')) throw new Error('This EPUB contains invalid XML.')
  return document
}

// ZIP indexing and page extraction use bounded ranges, never a full archive Blob.
export const MAX_CBZ_BYTES = 4 * 1024 ** 3
export const MAX_CACHE_BYTES = 64 * 1024 ** 2
const MAX_INDEX_READ = 16 * 1024 ** 2
const RANGE_CHUNK = 512 * 1024
export class BookDeliveryError extends Error {}
class RetryableRangeError extends BookDeliveryError {}

async function range(url:string,start:number,length:number,signal:AbortSignal,total?:number,validator?:string){
  const headers:Record<string,string>={Range:`bytes=${start}-${start+length-1}`}
  if(validator)headers['If-Range']=validator
  const response=await fetch(url,{signal,headers,credentials:'same-origin',cache:'no-store'}).catch(error=>{
    signal.throwIfAborted()
    throw new RetryableRangeError('The book source could not be reached.', {cause:error})
  })
  if([502,503,504].includes(response.status)){
    await response.body?.cancel()
    throw new RetryableRangeError('The book source is temporarily unavailable.')
  }
  const contentRange=/^bytes (\d+)-(\d+)\/(\d+)$/.exec(response.headers.get('content-range')??'')
  if(response.status!==206||!response.body||!contentRange||Number(contentRange[1])!==start||Number(contentRange[2])!==start+length-1||!Number.isSafeInteger(Number(contentRange[3]))||(total!==undefined&&Number(contentRange[3])!==total)){
    await response.body?.cancel()
    throw new BookDeliveryError('This book could not be read with byte ranges. Its source may be unavailable, the file may have changed, or a proxy may be stripping Range headers.')
  }
  const bytes=new Uint8Array(length),stream=response.body.getReader()
  let received=0
  try{while(true){const {value,done}=await stream.read().catch(error=>{signal.throwIfAborted();throw new RetryableRangeError('The book source interrupted the range.',{cause:error})});if(done)break;if(received+value.byteLength>length)throw Error('The book source returned an oversized range.');bytes.set(value,received);received+=value.byteLength}}
  finally{await stream.cancel().catch(()=>{})}
  if(received!==length)throw new RetryableRangeError('The book source returned an incomplete range.')
  return {bytes,size:Number(contentRange[3]),validator:response.headers.get('etag')?.replace(/^W\/.*$/,'')||response.headers.get('last-modified')||undefined}
}
class RangeReader extends Reader<undefined>{
  constructor(private url:string,size:number,private signal:AbortSignal,private validator:string|undefined,private retryRanges:boolean){super(undefined);this.size=size}
  async readUint8Array(index:number,length:number){
    this.signal.throwIfAborted()
    if(!Number.isSafeInteger(index)||index<0||!Number.isSafeInteger(length)||length<0)throw Error('Invalid book byte range.')
    length=Math.min(length,Math.max(0,this.size-index))
    if(length>MAX_INDEX_READ)throw Error('This archive exceeds the ZIP index processing limit.')
    const bytes=new Uint8Array(length)
    for(let offset=0;offset<length;offset+=RANGE_CHUNK){
      const readChunk=()=>range(this.url,index+offset,Math.min(RANGE_CHUNK,length-offset),this.signal,this.size,this.validator)
      const chunk=await readChunk().catch(error=>{
        this.signal.throwIfAborted()
        if(!this.retryRanges||!(error instanceof RetryableRangeError))throw error
        return readChunk()
      })
      bytes.set(chunk.bytes,offset)
    }
    return bytes
  }
}
export async function openArchive(url: string, signal: AbortSignal, format:'epub'|'cbz'='epub') {
  const lifetime=new AbortController()
  const abort=()=>lifetime.abort(signal.reason)
  if(signal.aborted)abort();else signal.addEventListener('abort',abort,{once:true})
  const stop=()=>{lifetime.abort();signal.removeEventListener('abort',abort)}
  let reader:ZipReader<undefined>|undefined
  try {
    const initial=await range(url,0,1,lifetime.signal)
    const maxBytes=format==='cbz'?MAX_CBZ_BYTES:MAX_BOOK_BYTES
    if(initial.size>maxBytes)throw Error(format==='cbz'?'CBZ archives must be at most 4 GiB.':'EPUB books must be at most 128 MiB.')
    reader = new ZipReader(new RangeReader(url,initial.size,lifetime.signal,initial.validator,format==='cbz'), { useWebWorkers: false })
    const entries = new Map<string, FileEntry>()
    let expanded = 0
    let entryCount = 0
    for await (const entry of reader.getEntriesGenerator()) {
      if (++entryCount > 5000) throw new Error('This archive contains too many files.')
      if (entry.directory) continue
      expanded += entry.uncompressedSize
      const maxExpanded=format==='cbz'?8*1024**3:MAX_EXPANDED_BYTES
      if (entry.encrypted || !Number.isSafeInteger(entry.uncompressedSize) || entry.uncompressedSize<0 || entry.uncompressedSize > MAX_ENTRY_BYTES || expanded > maxExpanded) throw new Error('This book is encrypted or exceeds the reader archive limits.')
      if (entries.has(entry.filename)) throw new Error('This archive contains duplicate file names.')
      entries.set(entry.filename, entry)
    }
    let cacheBytes=0,slots=2
    let retained: ReadonlySet<string> | undefined
    const cache = new Map<string, Blob>(),pending=new Map<string,Promise<Blob>>()
    const waiters:(()=>void)[]=[]
    const acquire=async()=>{if(slots>0){slots--;return}await new Promise<void>(resolve=>waiters.push(resolve))}
    const release=()=>{const next=waiters.shift();if(next)next();else slots++}
    const extract = async (name: string) => {
      await acquire()
      try{
        lifetime.signal.throwIfAborted()
        const entry = entries.get(name)
        if (!entry) throw new Error('A required book resource is missing.')
        const parts: Uint8Array<ArrayBuffer>[] = []
        let size = 0
        await entry.getData(new WritableStream<Uint8Array>({ write(chunk) {
          size += chunk.byteLength
          if (size > MAX_ENTRY_BYTES || size>entry.uncompressedSize) throw new Error('A book resource exceeds the reader size limit.')
          parts.push(new Uint8Array(chunk))
        } }), { signal:lifetime.signal, checkSignature: true })
        if(size!==entry.uncompressedSize)throw Error('A book resource has an invalid size.')
        return new Blob(parts)
      }finally{release()}
    }
    const read = (name: string):Promise<Blob> => {
      if(lifetime.signal.aborted)return Promise.reject(lifetime.signal.reason)
      const cached=cache.get(name)
      if(cached){cache.delete(name);cache.set(name,cached);return Promise.resolve(cached)}
      let result=pending.get(name)
      if(!result){
        result=extract(name).then(blob=>{
          if(!lifetime.signal.aborted&&(!retained||retained.has(name))){
            while(cacheBytes+blob.size>MAX_CACHE_BYTES&&cache.size){const oldest=cache.keys().next().value!;cacheBytes-=cache.get(oldest)!.size;cache.delete(oldest)}
            cache.set(name,blob);cacheBytes+=blob.size
          }
          return blob
        }).finally(()=>pending.delete(name))
        pending.set(name,result)
      }
      return result
    }
    const retain = (names: ReadonlySet<string>) => {
      retained=names
      for(const [name,blob] of cache){
        if(names.has(name))continue
        cache.delete(name)
        cacheBytes-=blob.size
      }
    }
    const opened=reader
    return { entries, read, retain, close: async()=>{stop();cache.clear();cacheBytes=0;await opened.close()} }
  } catch (error) { stop();await reader?.close();throw error }
}
export type BookArchive = Awaited<ReturnType<typeof openArchive>>
