import { expect, it, vi } from 'vitest'
import { ProgressSession } from './progress'
const position = (value: number) => ({ position: value, total: 100, locator: null })
it('loads a revision and serializes/coalesces writes with the next returned revision', async () => {
  let finish: (response: Response) => void = () => {}
  const fetcher = vi.fn().mockResolvedValueOnce(new Response(JSON.stringify({ ...position(12), revision: 4 }))).mockImplementationOnce(() => new Promise<Response>(resolve => { finish = resolve })).mockResolvedValueOnce(new Response(JSON.stringify({ ...position(30), revision: 6 })))
  const error = vi.fn(), session = new ProgressSession('source','file.mp4',error,fetcher)
  expect((await session.load())?.position).toBe(12)
  session.save(position(20)); session.save(position(25)); session.save(position(30))
  expect(fetcher).toHaveBeenCalledTimes(2)
  finish(new Response(JSON.stringify({ ...position(20), revision: 5 })))
  await vi.waitFor(() => expect(fetcher).toHaveBeenCalledTimes(3))
  const payload = JSON.parse(fetcher.mock.calls[2][1].body)
  expect(payload.position).toBe(30); expect(payload.expected_revision).toBe(5); expect(error).not.toHaveBeenCalled()
})
it('never overwrites progress after a conflicting player write', async () => {
  const fetcher = vi.fn().mockResolvedValueOnce(new Response('null')).mockResolvedValueOnce(new Response('',{status:409}))
  const error = vi.fn(), session = new ProgressSession('source','file',error,fetcher)
  await session.load(); session.save(position(2))
  await vi.waitFor(() => expect(error).toHaveBeenCalled())
  session.save(position(3)); expect(fetcher).toHaveBeenCalledTimes(2)
})
it('does not save an unknown revision after a failed initial read', async () => {
  const fetcher = vi.fn().mockResolvedValue(new Response('',{status:503})), error = vi.fn()
  const session = new ProgressSession('source','file',error,fetcher)
  await session.load(); session.save(position(3)); expect(fetcher).toHaveBeenCalledTimes(1); expect(error).toHaveBeenCalled()
})
