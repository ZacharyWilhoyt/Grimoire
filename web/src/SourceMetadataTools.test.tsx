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
