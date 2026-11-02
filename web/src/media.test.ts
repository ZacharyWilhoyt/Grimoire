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
it('orders album members while keeping source ownership and creator identities separate', async () => {
  const { groupMedia } = await import('./media')
  const item = (id: string, source: string, track: number, creator = 'Artist') => ({ ...file(id, 'music', source), metadata: { fields: { title: id, creator, collection: 'Album', track }, has_custom: false, origin: 'local', artwork_url: null, can_edit: false, artwork_preference: 'auto', provider: null, provider_id: null } })
  const groups = groupMedia([item('ten','one',10),item('two','one',2),item('other','one',1,'Other Artist'),item('separate','two',1)],'collection')
  expect(groups).toHaveLength(3)
  expect(groups[0].items.map(item => item.file_key)).toEqual(['two','ten'])
  expect(groups[1].items.map(item => item.file_key)).toEqual(['separate'])
  expect(groups[2].items.map(item => item.file_key)).toEqual(['other'])
})

it('consolidates series across seasons and exposes numbered season folders', async () => {
  const { groupMedia, subdivisions } = await import('./media')
  const episode = (key: string, season: number, episode: number): MediaItem => ({ ...file(key, 'video'), metadata: { fields: { collection: 'A Show', season, episode }, has_custom: false, origin: 'local', artwork_url: null, can_edit: true, artwork_preference: 'auto', provider: null, provider_id: null } })
  const groups = groupMedia([episode('s2e2',2,2),episode('s1e2',1,2),episode('s1e1',1,1)],'collection')
  expect(groups).toHaveLength(1)
  expect(groups[0].folder).toBe(true)
  expect(subdivisions(groups[0].items)?.map(group => [group.label, group.items.map(item => item.file_key)])).toEqual([['Season 1',['s1e1','s1e2']],['Season 2',['s2e2']]])
})
it('leaves unrelated files visible and does not invent collection folders', async () => {
  const { groupMedia } = await import('./media')
  const groups = groupMedia([file('standalone','book'),file('other','book')],'collection')
  expect(groups).toHaveLength(2)
  expect(groups.every(group => !group.folder)).toBe(true)
})

it('creates disc folders only for multiple known discs', async () => {
  const { subdivisions } = await import('./media')
  const track = (key: string, disc?: number): MediaItem => ({ ...file(key, 'music'), metadata: { fields: { collection: 'Album', disc }, has_custom: false, origin: 'local', artwork_url: null, can_edit: true, artwork_preference: 'auto', provider: null, provider_id: null } })
  expect(subdivisions([track('unknown'),track('one',1)])).toBeNull()
  expect(subdivisions([track('two',2),track('one',1)])?.map(group => group.label)).toEqual(['Disc 1','Disc 2'])
})

it('opens book series as one folder with volumes in series order', async () => {
  const { groupMedia } = await import('./media')
  const volume = (key: string, position: number): MediaItem => ({ ...file(key, 'book'), metadata: { fields: { title: key, creator: 'Author', collection: 'Series', volume: position }, has_custom: false, origin: 'local', artwork_url: null, can_edit: true, artwork_preference: 'auto', provider: null, provider_id: null } })
  const groups = groupMedia([volume('third',3),volume('first',1),volume('second',2)],'collection')
  expect(groups).toHaveLength(1)
  expect(groups[0].folder).toBe(true)
  expect(groups[0].items.map(item => item.file_key)).toEqual(['first','second','third'])
})
