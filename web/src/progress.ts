import { apiFetch } from './api'
export type Progress = { position: number; total: number | null; locator: string | null; revision: number }
export type Position = Omit<Progress, 'revision'>
// Each mounted reader/player gets its own revision owner. Never share it across users.
export class ProgressSession {
  private revision = 0
  private ready = false
  private failed = false
  private pending: Position | null = null
  private writing = false
  private drained: Promise<void> = Promise.resolve()
  constructor(private sourceId: string, private path: string, private onError: (message: string) => void, private fetcher = apiFetch) {}
  async load(): Promise<Progress | null> {
    try {
      const response = await this.fetcher(`/api/progress?${new URLSearchParams({ source_id: this.sourceId, relative_path: this.path })}`)
      if (!response.ok) throw new Error('Could not load saved progress. Playback is available, but progress saving is disabled for this session.')
      const data = await response.json() as Progress | null
      this.revision = data?.revision ?? 0; this.ready = true
      return data
    } catch (error) { this.failed = true; this.onError(error instanceof Error ? error.message : 'Could not load progress.'); return null }
  }
  save(position: Position) {
    if (!this.ready || this.failed) return
    this.pending = position
    if (!this.writing) this.drained = this.drain()
  }
  flush() { return this.drained }
  private async drain() {
    this.writing = true
    try {
      while (this.pending && !this.failed) {
        const position = this.pending; this.pending = null
        const response = await this.fetcher('/api/progress', { method: 'PUT', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ source_id: this.sourceId, relative_path: this.path, ...position, expected_revision: this.revision }), keepalive: true })
        if (!response.ok) throw new Error(response.status === 409 ? 'Progress changed in another player. Close and reopen this item to resume the latest position.' : 'Progress could not be saved. Close and reopen this item to reconnect.')
        const data = await response.json() as Progress
        this.revision = data.revision
      }
    } catch (error) { this.failed = true; this.pending = null; this.onError(error instanceof Error ? error.message : 'Could not save progress.') }
    finally { this.writing = false }
  }
}
