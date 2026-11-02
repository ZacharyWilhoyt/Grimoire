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

it('uses display metadata for search and genre filters while preserving original file identity', async () => {
  const { displayTitle } = await import('./media')
  const item = { ...file('original', 'book'), metadata: { fields: { title: 'A Better Title', creator: 'An Author', collection: 'A Series', genres: ['fantasy'] }, has_custom: true, origin: 'custom', artwork_url: null, can_edit: true, artwork_preference: 'auto', provider: null, provider_id: null } }
  expect(displayTitle(item)).toBe('A Better Title')
  expect(filterMedia([item], 'an author', '', '', 'title', 'fantasy')).toEqual([item])
  expect(filterMedia([item], '', '', '', 'title', 'horror')).toEqual([])
  expect(item.name).toBe('original.media')
  expect(item.file_key).toBe('original')
})
it('groups albums across accessible sources in disc and track order without merging identities', async () => {
  const { groupMedia } = await import('./media')
  const item = (id: string, source: string, track: number, creator = 'Artist') => ({ ...file(id, 'music', source), metadata: { fields: { title: id, creator, collection: 'Album', track }, has_custom: false, origin: 'local', artwork_url: null, can_edit: false, artwork_preference: 'auto', provider: null, provider_id: null } })
  const groups = groupMedia([item('ten','one',10),item('two','two',2),item('other','one',1,'Other Artist')],'collection')
  expect(groups).toHaveLength(2)
  expect(groups[0].items.map(item => item.file_key)).toEqual(['two','ten'])
  expect(groups[1].items.map(item => item.file_key)).toEqual(['other'])
})
