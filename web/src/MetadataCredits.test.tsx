import { expect, it } from 'vitest'
import { renderToStaticMarkup } from 'react-dom/server'
import MetadataCredits from './MetadataCredits'
it('credits music sources without claiming TMDB supplied music metadata', () => {
  const html = renderToStaticMarkup(<MetadataCredits credits={['musicbrainz','coverartarchive']}/>)
  expect(html).toContain('MusicBrainz')
  expect(html).toContain('Cover Art Archive')
  expect(html).not.toContain('TMDB')
  expect(html).not.toContain('Open Library')
})
it('includes TMDB attribution only in relevant item credits and general application credits', () => {
  expect(renderToStaticMarkup(<MetadataCredits credits={['tmdb']}/>)).toContain('not endorsed or certified by TMDB')
  expect(renderToStaticMarkup(<MetadataCredits/>)).toContain('Application credits')
  expect(renderToStaticMarkup(<MetadataCredits credits={[]}/>)).toBe('')
})
