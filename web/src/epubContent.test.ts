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

it('keeps paper as default and applies night colors without changing the isolation policy', () => {
  const paper = chapterDocument('<p>Text</p>', 19)
  const night = chapterDocument('<p>Text</p>', 19, 'night')
  expect(paper).toContain('background:#f5f0e6')
  expect(night).toContain('background:#18191d')
  expect(night).toContain('color-scheme:dark')
  expect(night).toContain("default-src 'none'")
  expect(night).toContain("img-src blob:")
  expect(chapterDocument('', NaN)).toContain('font-size:19px')
})
