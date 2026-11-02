import { expect, it } from 'vitest'
import { filterMedia, pageFromHash, suggestions, type MediaItem } from './media'
const file = (key: string, kind: MediaItem['kind'], source = 'one'): MediaItem => ({ file_key: key, available: true, source_id: source, source_name: source, name: `${key}.media`, relative_path: `folder/${key}.media`, kind, extension: 'media', size_bytes: 1, modified_at: 1, saved_at: null, last_opened_at: null })
it('filters real catalog entries by text, type, and source without mutating the list', () => {
  const items = [file('track', 'music'), file('movie', 'video'), file('book', 'book', 'two')]
  expect(filterMedia(items, ' TRACK ', 'music', 'one', 'title').map(item => item.file_key)).toEqual(['track'])
  expect(filterMedia(items, 'folder', '', 'two', 'title').map(item => item.file_key)).toEqual(['book'])
  expect(items.map(item => item.file_key)).toEqual(['track', 'movie', 'book'])
})
it('sorts saved items by save date and distinguishes files with the same name', () => {
  const items = [{ ...file('a', 'music'), saved_at: 2 }, { ...file('b', 'music'), saved_at: 8 }]
  expect(filterMedia(items, '', '', '', 'saved').map(item => item.file_key)).toEqual(['b', 'a'])
})
it('suggests all three media types and omits recently opened entries', () => {
  const items = [file('v1', 'video'), file('v2', 'video'), file('m', 'music'), file('b', 'book')]
  expect(suggestions(items, new Set(['v1']), 3).map(item => item.kind)).toEqual(['video', 'music', 'book'])
  expect(suggestions(items, new Set(['v1']), 3).map(item => item.file_key)).not.toContain('v1')
})
it('resolves the three bookmarkable navigation pages', () => {
  expect(pageFromHash('#discover')).toBe('Discover')
  expect(pageFromHash('#library')).toBe('My library')
  expect(pageFromHash('#for-you')).toBe('For you')
  expect(pageFromHash('#unknown')).toBe('For you')
})
