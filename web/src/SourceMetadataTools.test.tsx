// @vitest-environment jsdom
import { act } from 'react'
import { createRoot } from 'react-dom/client'
import { expect, it, vi } from 'vitest'
import SourceMetadataTools from './SourceMetadataTools'
const api = vi.hoisted(() => vi.fn())
vi.mock('./api', () => ({ apiFetch: api }))
it('stops the displayed job by ID and refreshes the final state without starting another scan', async () => {
  vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true)
  const running = { id: 'job-id', status: 'running', total: 10, processed: 3, skipped: 0, warnings: 0, message: null, stale: false }
  api.mockReset().mockResolvedValueOnce({ ok: true, json: async () => running }).mockResolvedValueOnce({ ok: true }).mockResolvedValueOnce({ ok: true, json: async () => ({ ...running, status: 'interrupted', message: 'Stopped by owner.' }) })
  const host = document.createElement('div'), root = createRoot(host), updated = vi.fn()
  try {
    await act(async () => root.render(<SourceMetadataTools sourceId="source" onUpdated={updated}/>))
    const stop = [...host.querySelectorAll('button')].find(button => button.textContent === 'Stop scan')!
    expect(stop).toBeDefined()
    await act(async () => stop.click())
    expect(api.mock.calls[1]).toEqual(['/api/sources/source/metadata/job/job-id/cancel', { method: 'POST' }])
    expect(host.textContent).toContain('Stopped by owner.')
    expect(host.textContent).not.toContain('Stop scan')
    expect(updated).toHaveBeenCalledOnce()
    expect(api.mock.calls.some(([url]) => String(url).endsWith('/scan'))).toBe(false)
  } finally { await act(async () => root.unmount()); vi.unstubAllGlobals() }
})
it('retries an unavailable scan status without submitting a scan mutation', async () => {
  vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true)
  api.mockReset().mockRejectedValueOnce(new Error('Scan status unavailable.')).mockResolvedValueOnce({ ok: true, json: async () => null })
  const host = document.createElement('div'), root = createRoot(host)
  try {
    await act(async () => root.render(<SourceMetadataTools sourceId="source" onUpdated={() => {}}/>))
    const gather = () => [...host.querySelectorAll('button')].find(button => button.textContent?.includes('Gather local metadata'))!
    expect(gather()!.disabled).toBe(true)
    expect(host.textContent).toContain('Scan status unavailable.')
    await act(async () => [...host.querySelectorAll('button')].find(button => button.textContent === 'Retry scan status')!.click())
    expect(gather()!.disabled).toBe(false)
    expect(host.textContent).not.toContain('Scan status unavailable.')
    expect(api.mock.calls).toHaveLength(2)
    expect(api.mock.calls.every(([url, options]) => String(url).endsWith('/metadata/job') && !options?.method)).toBe(true)
  } finally { await act(async () => root.unmount()); vi.unstubAllGlobals() }
})

it('labels stale display data after a polling failure and resumes status updates on retry', async () => {
  vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true); vi.useFakeTimers()
  const job = { id: 'job-id', status: 'running', total: 10, processed: 3, skipped: 0, warnings: 0, message: null, stale: false }
  api.mockReset().mockResolvedValueOnce({ ok: true, json: async () => job }).mockRejectedValueOnce(new Error('Connection interrupted.')).mockResolvedValueOnce({ ok: true, json: async () => ({ ...job, status: 'completed', processed: 10 }) })
  const host = document.createElement('div'), root = createRoot(host), updated = vi.fn()
  try {
    await act(async () => root.render(<SourceMetadataTools sourceId="source" onUpdated={updated}/>))
    await act(async () => { await vi.advanceTimersByTimeAsync(2500) })
    expect(host.textContent).toContain('Last known status: running: 3/10')
    expect([...host.querySelectorAll('button')].find(button => button.textContent === 'Stop scan')!.disabled).toBe(true)
    await act(async () => [...host.querySelectorAll('button')].find(button => button.textContent === 'Retry scan status')!.click())
    expect(host.textContent).toContain('completed: 10/10')
    expect(host.textContent).not.toContain('Last known status')
    expect(host.textContent).not.toContain('Connection interrupted.')
    expect(updated).toHaveBeenCalledOnce()
  } finally { await act(async () => root.unmount()); vi.useRealTimers(); vi.unstubAllGlobals() }
})
