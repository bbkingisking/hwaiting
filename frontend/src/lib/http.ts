// Browsers manage the HttpOnly authentication cookie. Writes carry a
// non-safelisted header so cross-origin requests require CORS preflight.
export function fetchWithCookies(url: string, options: RequestInit = {}): Promise<Response> {
  const headers = new Headers(options.headers)
  const method = (options.method ?? 'GET').toUpperCase()
  if (!['GET', 'HEAD', 'OPTIONS'].includes(method)) {
    headers.set('X-Requested-With', 'hwaiting')
  }
  return fetch(url, { ...options, headers, credentials: 'include' })
}
