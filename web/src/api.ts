// Session cookies remain HttpOnly; only the anti-CSRF token lives in JS memory.
let csrfToken = ''
export function setCsrfToken(token: string) { csrfToken = token }
export async function apiFetch(input: string, init: RequestInit = {}) {
  const target = new URL(input, window.location.origin)
  if (target.origin !== window.location.origin || !target.pathname.startsWith('/api/')) throw new Error('API requests must stay on the application origin.')
  const headers = new Headers(init.headers)
  if (!['GET', 'HEAD', 'OPTIONS'].includes((init.method ?? 'GET').toUpperCase()) && csrfToken) headers.set('X-CSRF-Token', csrfToken)
  const response = await fetch(input, { ...init, headers, credentials: 'same-origin', cache: 'no-store' })
  if (response.status === 401) window.dispatchEvent(new Event('grimoire-session-expired'))
  return response
}
