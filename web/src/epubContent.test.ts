// @vitest-environment jsdom
import { describe, expect, it } from 'vitest'
import { chapterDocument, sanitizeChapter } from './epubContent'

describe('EPUB isolation', () => {
  it('removes executable content while preserving readable markup', () => {
    const fragment = sanitizeChapter('<h1>Chapter</h1><p>Hello</p><script>alert(1)</script><iframe src="https://example.com"></iframe><img onerror="alert(1)" srcset="https://example.com/x 2x"><style>body{display:none}</style><form><input></form><a href="javascript:alert(1)">bad link</a>')
    const wrapper = document.createElement('div'); wrapper.append(fragment)
    expect(wrapper.querySelector('h1')?.textContent).toBe('Chapter')
    expect(wrapper.querySelector('script,iframe,style,form,input')).toBeNull()
    expect(wrapper.querySelector('[onerror],[srcset],[href]')).toBeNull()
  })
  it('blocks external resource loads and bounds text sizing', () => {
    const document = chapterDocument('<p>Text</p>', 999)
    expect(document).toContain("default-src 'none'")
    expect(document).toContain("img-src blob:")
    expect(document).toContain("form-action 'none'")
    expect(document).toContain('font-size:30px')
  })
})
