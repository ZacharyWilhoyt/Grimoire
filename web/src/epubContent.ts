import DOMPurify from 'dompurify'

export function sanitizeChapter(markup: string) {
  return DOMPurify.sanitize(markup, {
    USE_PROFILES: { html: true }, RETURN_DOM_FRAGMENT: true,
    FORBID_TAGS: ['style', 'link', 'iframe', 'form', 'input', 'video', 'audio', 'source', 'object', 'embed'],
    FORBID_ATTR: ['style', 'srcset', 'target'],
  })
}

export function chapterDocument(content: string, fontSize: number) {
  const size = Math.max(14, Math.min(30, fontSize))
  return `<!doctype html><html><head><meta http-equiv="Content-Security-Policy" content="default-src 'none'; img-src blob:; style-src 'unsafe-inline'; base-uri 'none'; form-action 'none'"><style>body{max-width:720px;margin:auto;padding:28px;font-family:Georgia,serif;font-size:${size}px;line-height:1.8;color:#292525;background:#f5f0e6}img{max-width:100%;height:auto}pre{white-space:pre-wrap}*{overflow-wrap:anywhere}</style></head><body>${content}</body></html>`
}
