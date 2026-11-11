// @vitest-environment jsdom
import { act } from 'react'
import { createRoot } from 'react-dom/client'
import { expect, it, vi } from 'vitest'
import BookReader from './BookReader'
vi.mock('./MediaComments', () => ({ default: () => null }))
const session = vi.hoisted(() => ({ save: vi.fn() }))
vi.mock('./progress', () => ({ ProgressSession: class { load = async () => null; save = session.save } }))
const comicFixture = vi.hoisted(() => ({
  paths: ['1.png', '2.png'],
  read: vi.fn(async (_path: string) => new Blob(['image'])),
  retain: vi.fn(),
}))
const epubFiles = vi.hoisted((): Record<string, string> => ({
  'META-INF/container.xml': '<container><rootfiles><rootfile full-path="OPS/package.opf"/></rootfiles></container>',
  'OPS/package.opf': '<package><manifest><item id="chapter" href="one.xhtml" media-type="application/xhtml+xml"/><item id="nav" href="nav.xhtml" properties="nav"/></manifest><spine><itemref idref="chapter"/></spine></package>',
  'OPS/nav.xhtml': '<html xmlns:epub="http://www.idpf.org/2007/ops"><body><nav epub:type="toc"><a href="one.xhtml">An actual chapter title</a></nav></body></html>',
  'OPS/one.xhtml': '<html><body><p>Chapter content</p></body></html>',
}))
vi.mock('./bookArchive', async importOriginal => ({
  ...await importOriginal<object>(),
  openArchive: async (url: string) => url.includes('epub') ? {
    entries: new Map(),
    read: async (path: string) => ({ text: async () => epubFiles[path] ?? '' }),
    close: async () => {},
  } : {
    entries: new Map(comicFixture.paths.map(path => [path, {}])),
    read: comicFixture.read,
    retain: comicFixture.retain,
    close: async () => {},
  },
}))

it('saves a comic page only after its image loads and does not save a damaged image', async () => {
  vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true)
  Object.defineProperty(HTMLDialogElement.prototype, 'showModal', { configurable: true, value: vi.fn() })
  Object.defineProperty(HTMLDialogElement.prototype, 'close', { configurable: true, value: vi.fn() })
  URL.createObjectURL = vi.fn(() => 'blob:test'); URL.revokeObjectURL = vi.fn()
  session.save.mockClear()
  const host = document.createElement('div'), root = createRoot(host)
  try {
    await act(async () => root.render(<BookReader sourceId="source" sourceName="Test" file={{ name: 'Comic', relative_path: 'comic.cbz', extension: 'cbz' }} onClose={() => {}}/>))
    const first = host.querySelector('img')!
    expect(first).not.toBeNull(); expect(session.save).not.toHaveBeenCalled()
    await act(async () => first.dispatchEvent(new Event('load')))
    expect(session.save).toHaveBeenLastCalledWith({ position: 0, total: 2, locator: '1.png' })
    session.save.mockClear()
    await act(async () => host.querySelector<HTMLButtonElement>('button[aria-label="Next"]')!.click())
    expect(session.save).toHaveBeenLastCalledWith(expect.objectContaining({position:0,completed_units:[0]}))
    session.save.mockClear()
    await act(async () => host.querySelector('img')!.dispatchEvent(new Event('error')))
    expect(host.textContent).toContain('damaged or unsupported')
    expect(session.save).not.toHaveBeenCalled()
  } finally { await act(async () => root.unmount()); vi.restoreAllMocks(); vi.unstubAllGlobals() }
})

it('shows EPUB navigation titles while preserving chapter progress locators', async () => {
  vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true)
  Object.defineProperty(HTMLDialogElement.prototype, 'showModal', { configurable: true, value: vi.fn() })
  Object.defineProperty(HTMLDialogElement.prototype, 'close', { configurable: true, value: vi.fn() })
  session.save.mockClear()
  const host = document.createElement('div'), root = createRoot(host)
  try {
    await act(async () => root.render(<BookReader sourceId="source" sourceName="Test" file={{ name: 'Book', relative_path: 'book.epub', extension: 'epub' }} onClose={() => {}}/>))
    expect(host.querySelector('select')?.textContent).toContain('An actual chapter title')
    expect(host.querySelector('iframe')?.getAttribute('sandbox')).toBe('allow-same-origin')
    expect(session.save).toHaveBeenLastCalledWith({ position: 0, total: 1, locator: 'OPS/one.xhtml' })
    await act(async()=>host.querySelector<HTMLButtonElement>('[aria-label="Reading settings"]')!.click())
    const theme = host.querySelector<HTMLSelectElement>('select[aria-label="Reading theme"]')!
    expect(theme.value).toBe('paper')
    session.save.mockClear()
    await act(async () => { theme.value = 'night'; theme.dispatchEvent(new Event('change', { bubbles: true })) })
    expect(host.querySelector('iframe')?.getAttribute('srcdoc')).toContain('background:#18191d')
    expect(host.querySelector('iframe')?.getAttribute('sandbox')).toBe('allow-same-origin')
    expect(session.save).not.toHaveBeenCalled()
  } finally { await act(async () => root.unmount()); vi.restoreAllMocks(); vi.unstubAllGlobals() }
})

it('toggles cozy view without replacing content and separates profile preferences', async()=>{
 vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT',true)
 localStorage.clear()
 URL.createObjectURL=vi.fn(()=>'blob:test');URL.revokeObjectURL=vi.fn()
 const host=document.createElement('div'),root=createRoot(host)
 try{
  await act(async()=>root.render(<BookReader sourceId="s" sourceName="Test" file={{name:'Comic',relative_path:'comic.cbz',extension:'cbz'}} onClose={()=>{}}/>))
  const image=host.querySelector('img')!
  await act(async()=>host.querySelector('.reader-viewport')!.dispatchEvent(new MouseEvent('dblclick',{bubbles:true})))
  expect(host.querySelector('.watch-cozy')).not.toBeNull()
  expect(host.querySelector('img')).toBe(image)
  await act(async()=>host.querySelector<HTMLButtonElement>('[aria-label="Reading settings"]')!.click())
  expect(host.querySelector('.reader-margin')).toBeNull()
  expect(host.querySelector('[aria-label="Reader zoom"]')).toBeNull()
  const fit=host.querySelector<HTMLSelectElement>('[aria-label="Page fit"]')!
  await act(async()=>{fit.value='width';fit.dispatchEvent(new Event('change',{bubbles:true}))})
  const type=host.querySelector<HTMLSelectElement>('[aria-label="Content type"]')!
  await act(async()=>{type.value='Manga';type.dispatchEvent(new Event('change',{bubbles:true}))})
  expect(fit.value).toBe('both')
  await act(async()=>{type.value='Comic';type.dispatchEvent(new Event('change',{bubbles:true}))})
  expect(fit.value).toBe('width')
 }finally{await act(async()=>root.unmount());localStorage.clear();vi.unstubAllGlobals()}
})

it('continues to the next authorized series entry at the end',async()=>{
 vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT',true)
 const api=await import('./api')
 const next={file_key:'next',source_id:'s',relative_path:'next.cbz',name:'Next',kind:'book',available:true}
 const fetch=vi.spyOn(api,'apiFetch').mockImplementation(async()=>new Response(JSON.stringify({members:[{file_key:'current',source_id:'s',relative_path:'comic.cbz'},next]})))
 URL.createObjectURL=vi.fn(()=>'blob:test');URL.revokeObjectURL=vi.fn()
 const host=document.createElement('div'),root=createRoot(host),advance=vi.fn()
 try{
  await act(async()=>root.render(<BookReader sourceId="s" sourceName="Test" file={{name:'Comic',relative_path:'comic.cbz',extension:'cbz'}} onClose={()=>{}} onNext={advance}/>))
  const button=()=>host.querySelector<HTMLButtonElement>('[aria-label="Next"]')!
  await act(async()=>button().click())
  expect(advance).not.toHaveBeenCalled()
  await act(async()=>button().click())
  expect(advance).toHaveBeenCalledExactlyOnceWith(next)
 }finally{await act(async()=>root.unmount());fetch.mockRestore();vi.unstubAllGlobals()}
})

it('keeps the content point under the cursor fixed when zooming',async()=>{
 vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT',true)
 localStorage.clear()
 URL.createObjectURL=vi.fn(()=>'blob:test');URL.revokeObjectURL=vi.fn()
 const host=document.createElement('div'),root=createRoot(host)
 document.body.append(host)
 try{
  await act(async()=>root.render(<BookReader sourceId="s" sourceName="Test" file={{name:'Comic',relative_path:'comic.cbz',extension:'cbz'}} onClose={()=>{}}/>))
  const view=host.querySelector<HTMLElement>('.reader-viewport')!,image=host.querySelector('img')!
  image.getBoundingClientRect=()=>{const zoom=Number((host.querySelector<HTMLElement>('.reader-surface')!).style.zoom);return {left:100,top:50,width:200*zoom,height:300*zoom,right:100+200*zoom,bottom:50+300*zoom,x:100,y:50,toJSON:()=>({})}}
  await act(async()=>view.dispatchEvent(new WheelEvent('wheel',{bubbles:true,cancelable:true,shiftKey:true,deltaY:-100,clientX:150,clientY:100})))
  expect(Number(host.querySelector<HTMLElement>('.reader-surface')!.style.zoom)).toBeCloseTo(1.1)
  expect(view.scrollLeft).toBeCloseTo(5)
  expect(view.scrollTop).toBeCloseTo(5)
  expect(host.querySelector('[aria-label="Page navigation"]')?.closest('header')).not.toBeNull()
  expect(host.querySelector('.reader-margin')).toBeNull()
 }finally{await act(async()=>root.unmount());host.remove();localStorage.clear();vi.unstubAllGlobals()}
})


it('preloads nearby pages and waits for an uncached jump before showing the image', async () => {
  vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true)
  URL.createObjectURL = vi.fn(() => 'blob:test')
  URL.revokeObjectURL = vi.fn()
  comicFixture.paths = Array.from({ length: 9 }, (_, i) => `${i+1}.png`)
  comicFixture.read.mockClear()
  let loadPage!: (page: Blob) => void
  comicFixture.read.mockImplementation(path => path === '7.png'
    ? new Promise(resolve => { loadPage = resolve })
    : Promise.resolve(new Blob(['image'])))
  const host = document.createElement('div'), root = createRoot(host)
  try {
    await act(async () => root.render(<BookReader sourceId="source" sourceName="Test" file={{ name: 'Comic', relative_path: 'comic.cbz', extension: 'cbz' }} onClose={() => {}}/>))
    expect(comicFixture.read.mock.calls).toEqual([['1.png'], ['2.png'], ['3.png']])
    const select = host.querySelector<HTMLSelectElement>('[aria-label="Page"]')!
    await act(async () => { select.value = '6'; select.dispatchEvent(new Event('change', { bubbles: true })) })
    expect(host.querySelector('.reader-page-status[role="status"]')?.textContent).toBe('Loading page 7…')
    expect(host.querySelector('[role="alert"]')).toBeNull()
    expect(comicFixture.retain).toHaveBeenLastCalledWith(new Set(['5.png','6.png','7.png','8.png','9.png']))
    await act(async () => loadPage(new Blob(['image'])))
    const image = host.querySelector<HTMLImageElement>('img[alt="Page 7"]')!
    expect(image).not.toBeNull()
    await act(async () => image.dispatchEvent(new Event('load')))
    expect(host.querySelector('.reader-page-status[role="status"]')).toBeNull()
  } finally {
    await act(async () => root.unmount())
    comicFixture.paths = ['1.png', '2.png']
    comicFixture.read.mockImplementation(async () => new Blob(['image']))
    vi.unstubAllGlobals()
  }
})

it('advances from the right margin in a right-to-left spread', async () => {
  vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true)
  URL.createObjectURL = vi.fn(() => 'blob:test')
  URL.revokeObjectURL = vi.fn()
  localStorage.setItem('grimoire:reader:v1:margin-test:Comic', JSON.stringify({ layout: 'double', direction: 'rtl' }))
  const host = document.createElement('div'), root = createRoot(host)
  try {
    await act(async () => root.render(<BookReader userId="margin-test" sourceId="s" sourceName="Test" file={{ name: 'Comic', relative_path: 'comic.cbz', extension: 'cbz' }} onClose={() => {}}/>))
    expect(host.querySelector<HTMLElement>('.reader-surface')!.style.direction).toBe('rtl')
    const viewport = host.querySelector<HTMLElement>('.reader-viewport')!
    vi.spyOn(viewport, 'getBoundingClientRect').mockReturnValue({ left: 0, width: 800 } as DOMRect)
    const page = host.querySelector<HTMLSelectElement>('[aria-label="Page"]')!
    vi.useFakeTimers()
    for (const [clientX, expectedPage] of [[790, '1'], [10, '0']] as const) {
      await act(async () => {
        viewport.dispatchEvent(new MouseEvent('click', { bubbles: true, clientX }))
        await vi.advanceTimersByTimeAsync(220)
      })
      expect(page.value).toBe(expectedPage)
    }
  } finally {
    await act(async () => root.unmount())
    vi.useRealTimers()
    localStorage.removeItem('grimoire:reader:v1:margin-test:Comic')
    vi.unstubAllGlobals()
  }
})

it('anchors zoom to the entire spread even when the cursor is over one page', async () => {
  vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true)
  vi.stubGlobal('Image', class {
    naturalWidth = 400
    naturalHeight = 700
    onload?: () => void
    set src(_value: string) { queueMicrotask(() => this.onload?.()) }
  })
  URL.createObjectURL = vi.fn(() => 'blob:test')
  URL.revokeObjectURL = vi.fn()
  const settingsKey = 'grimoire:reader:v1:spread-test:Comic'
  localStorage.setItem(settingsKey, JSON.stringify({ layout: 'double', fit: 'original' }))
  const host = document.createElement('div'), root = createRoot(host)
  document.body.append(host)
  try {
    await act(async () => root.render(<BookReader userId="spread-test" sourceId="s" sourceName="Test" file={{ name: 'Comic', relative_path: 'comic.cbz', extension: 'cbz' }} onClose={() => {}}/>))
    const image = host.querySelector('img')!
    Object.defineProperties(image, { naturalWidth: { value: 600 }, naturalHeight: { value: 800 } })
    await act(async () => image.dispatchEvent(new Event('load')))
    const spread = host.querySelector<HTMLElement>('.reader-spread')!
    expect(spread.querySelectorAll('.reader-page')).toHaveLength(2)
    // A 600 × 800 page is scaled to 525 × 700 to align with the 400 × 700 page.
    expect(spread.style.width).toContain('925px')
    expect(spread.style.gridTemplateColumns).toContain(`${100*.75/(.75+400/700)}fr`)
    const initialWidth = spread.style.width
    const viewport = host.querySelector<HTMLElement>('.reader-viewport')!
    const rect = (width: number, height: number) => ({ left: 0, top: 0, width, height, right: width, bottom: height }) as DOMRect
    image.getBoundingClientRect = () => rect(600, 800)
    spread.getBoundingClientRect = () => spread.style.width === initialWidth ? rect(925, 700) : rect(1017.5, 770)
    await act(async () => viewport.dispatchEvent(new WheelEvent('wheel', { bubbles: true, cancelable: true, shiftKey: true, deltaY: -100, clientX: 250, clientY: 200 })))
    expect(spread.style.width).not.toBe(initialWidth)
    expect(viewport.scrollLeft).toBeCloseTo(25)
    expect(viewport.scrollTop).toBeCloseTo(20)
  } finally {
    await act(async () => root.unmount())
    host.remove()
    localStorage.removeItem(settingsKey)
    vi.unstubAllGlobals()
  }
})
