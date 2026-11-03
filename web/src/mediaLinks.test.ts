import { expect, it } from 'vitest'
import { linkRequest, mediaLink, seriesLink } from './mediaLinks'
it('round trips media paths and series IDs without placing credentials in URLs', () => {
  const path='Series & Show/日本語 #1.mkv'
  expect(linkRequest(mediaLink('source 1',path).slice(1))).toBe(`/api/media/link?${new URLSearchParams({source_id:'source 1',path})}`)
  expect(linkRequest(seriesLink('source','abc').slice(1))).toBe('/api/media/link?source_id=source&group=abc')
  expect(linkRequest('')).toBeNull()
})
