// @vitest-environment jsdom
import { act } from 'react'
import { createRoot } from 'react-dom/client'
import { expect, it, vi } from 'vitest'
import * as api from './api'
import MediaComments from './MediaComments'

it('renders comments as text and posts a reply to the selected comment', async () => {
  vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true)
  const existing = { id: '1', parent_id: null, author: 'reader', reply_to: null, body: '<b>Hello</b>', created_at: 1700000000, removed: false }
  const reply = { ...existing, id: '2', parent_id: '1', reply_to: 'reader', author: 'viewer', body: 'My reply' }
  const fetch = vi.spyOn(api, 'apiFetch')
    .mockResolvedValueOnce(Response.json({ comments: [existing], next: null, can_moderate: true }))
    .mockResolvedValueOnce(Response.json(reply, { status: 201 }))
    .mockResolvedValueOnce(new Response(null, { status: 204 }))
  const confirm = vi.spyOn(window, 'confirm').mockReturnValue(true)
  const host = document.createElement('div')
  document.body.append(host)
  const root = createRoot(host)
  try {
    await act(async () => root.render(<MediaComments sourceId="source" path="episode.mkv"/>))
    expect(host.querySelector('.comment-list p')?.textContent).toBe('<b>Hello</b>')
    expect(host.querySelector('.comment-list p b')).toBeNull()
    const timestamp = host.querySelector('time')!
    const date = new Date(existing.created_at * 1000)
    expect(timestamp.dateTime).toBe(date.toISOString())
    expect(timestamp.textContent).toBe(date.toLocaleString(undefined, {
      year: 'numeric', month: 'numeric', day: 'numeric', hour: 'numeric', minute: '2-digit',
    }))
    await act(async () => host.querySelector<HTMLButtonElement>('[aria-label="Reply to reader"]')!.click())
    const input = host.querySelector<HTMLTextAreaElement>('textarea')!
    expect(document.activeElement).toBe(input)
    await act(async () => {
      Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, 'value')!.set!.call(input, 'My reply')
      input.dispatchEvent(new Event('input', { bubbles: true }))
    })
    await act(async () => host.querySelector('form')!.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true })))
    expect(JSON.parse(fetch.mock.calls[1][1]?.body as string)).toEqual({ source_id: 'source', path: 'episode.mkv', body: 'My reply', parent_id: '1' })
    expect(host.querySelector('.comment-reply p')?.textContent).toBe('My reply')
    expect(host.querySelector('.comment-parent')?.textContent).toBe('In reply to reader')
    await act(async () => host.querySelector<HTMLButtonElement>('[aria-label="Delete comment by reader"]')!.click())
    expect(fetch.mock.calls[2][0]).toBe('/api/media/comments/1?source_id=source&path=episode.mkv')
    expect(fetch.mock.calls[2][1]?.method).toBe('DELETE')
    expect(host.textContent).not.toContain('<b>Hello</b>')
    expect(host.querySelector('.comment-reply p')?.textContent).toBe('My reply')
    expect(host.querySelector('.comment-parent')?.textContent).toBe('In reply to Removed comment')
  } finally {
    await act(async () => root.unmount())
    host.remove()
    confirm.mockRestore()
    fetch.mockRestore()
    vi.unstubAllGlobals()
  }
})
