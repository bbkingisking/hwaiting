// API module for card operations and user management

interface CardResponse {
  word_id: number
  form: string
  hint: string
  context: string
  context_translation: string
  grammar: string | null
  politeness: string | null
  notes: string[]
}

interface ReviewRequest {
  rating: number // 1 = Again, 3 = Good
}

interface ReviewResponse {
  success: boolean
}

interface LanguageInfo {
  id: number
  code: string
  name: string
}

interface UserProfile {
  username: string
  target_language: LanguageInfo | null
}

interface SetLanguageResponse {
  success: boolean
}

class ApiError extends Error {
  constructor(public status: number, message: string) {
    super(message)
    this.name = 'ApiError'
  }
}

async function fetchWithAuth(url: string, options: RequestInit = {}) {
  const token = localStorage.getItem('annyeong-token')
  
  if (!token) {
    throw new ApiError(401, 'Not authenticated')
  }

  const response = await fetch(url, {
    ...options,
    headers: {
      ...options.headers,
      'Authorization': `Bearer ${token}`,
      'Content-Type': 'application/json',
    },
  })

  if (!response.ok) {
    const error = await response.json().catch(() => ({ error: 'Unknown error' }))
    throw new ApiError(response.status, error.error || `HTTP ${response.status}`)
  }

  return response.json()
}

export async function getNextCard(): Promise<CardResponse> {
  const url = `${window.location.origin}/api/cards/next`
  return fetchWithAuth(url)
}

export async function submitReview(wordId: number, rating: number): Promise<ReviewResponse> {
  const url = `${window.location.origin}/api/cards/${wordId}/review`
  return fetchWithAuth(url, {
    method: 'POST',
    body: JSON.stringify({ rating }),
  })
}

export async function getUserProfile(): Promise<UserProfile> {
  const url = `${window.location.origin}/api/user/me`
  return fetchWithAuth(url)
}

export async function getLanguages(): Promise<LanguageInfo[]> {
  const url = `${window.location.origin}/api/languages`
  const response = await fetch(url, {
    headers: {
      'Content-Type': 'application/json',
    },
  })

  if (!response.ok) {
    const error = await response.json().catch(() => ({ error: 'Unknown error' }))
    throw new ApiError(response.status, error.error || `HTTP ${response.status}`)
  }

  return response.json()
}

export async function setUserLanguage(languageId: number): Promise<SetLanguageResponse> {
  const url = `${window.location.origin}/api/user/language`
  return fetchWithAuth(url, {
    method: 'POST',
    body: JSON.stringify({ language_id: languageId }),
  })
}

interface ImportResponse {
  success: boolean
  words_imported: number
  reviews_imported: number
}

export async function importUserData(file: File): Promise<ImportResponse> {
  const text = await file.text()
  const data = JSON.parse(text)
  
  const url = `${window.location.origin}/api/user/import`
  return fetchWithAuth(url, {
    method: 'POST',
    body: JSON.stringify(data),
  })
}

export async function exportUserData(): Promise<void> {
  const url = `${window.location.origin}/api/user/export`
  const data = await fetchWithAuth(url)
  
  // Create a blob and download it
  const blob = new Blob([JSON.stringify(data, null, 2)], { type: 'application/json' })
  const downloadUrl = window.URL.createObjectURL(blob)
  const link = document.createElement('a')
  link.href = downloadUrl
  link.download = `annyeong-export-${new Date().toISOString().split('T')[0]}.json`
  document.body.appendChild(link)
  link.click()
  document.body.removeChild(link)
  window.URL.revokeObjectURL(downloadUrl)
}

export { ApiError }
export type { CardResponse, ReviewRequest, ReviewResponse, LanguageInfo, UserProfile, ImportResponse }