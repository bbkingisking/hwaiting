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
  correct_rate: number
  guess_count: number
  wrong_guess_count: number
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
    signal: options.signal,
  })

  if (!response.ok) {
    const error = await response.json().catch(() => ({ error: 'Unknown error' }))
    throw new ApiError(response.status, error.error || `HTTP ${response.status}`)
  }

  return response.json()
}

interface GetNextCardOptions {
  excludeWordId?: number
  signal?: AbortSignal
}

export async function getNextCard(options: GetNextCardOptions = {}): Promise<CardResponse> {
  const params = new URLSearchParams()
  if (options.excludeWordId !== undefined) {
    params.set('exclude', String(options.excludeWordId))
  }
  const qs = params.toString()
  const url = `${window.location.origin}/api/cards/next${qs ? `?${qs}` : ''}`
  return fetchWithAuth(url, { signal: options.signal })
}

export async function submitReview(wordId: number, rating: number): Promise<ReviewResponse> {
  const url = `${window.location.origin}/api/cards/${wordId}/review`
  return fetchWithAuth(url, {
    method: 'POST',
    body: JSON.stringify({ rating }),
  })
}

export async function suppressCard(wordId: number): Promise<ReviewResponse> {
  const url = `${window.location.origin}/api/cards/${wordId}/suppress`
  return fetchWithAuth(url, {
    method: 'PUT',
  })
}

interface SuppressedCard {
  word_id: number
  form: string
  hint: string
  context: string
  context_translation: string
  grammar: string | null
  politeness: string | null
}

interface SuppressedCardsResponse {
  cards: SuppressedCard[]
}

export async function listSuppressedCards(): Promise<SuppressedCardsResponse> {
  const url = `${window.location.origin}/api/cards/suppressed`
  return fetchWithAuth(url)
}

export async function unsuppressCard(wordId: number): Promise<ReviewResponse> {
  const url = `${window.location.origin}/api/cards/${wordId}/unsuppress`
  return fetchWithAuth(url, {
    method: 'PUT',
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

interface StatsResponse {
  new_count: number
  due_count: number
  reviews_today: number
  correct_today: number
  percentage: number | null
  next_due_at: string | null
}

export async function getStats(): Promise<StatsResponse> {
  const url = `${window.location.origin}/api/cards/stats`
  return fetchWithAuth(url)
}

interface UserSettings {
  show_percentage: boolean
  red_threshold: number
  yellow_threshold: number
  day_boundary_hour: number
  auto_progress_on_correct: boolean
  auto_progress_delay: number
  suppress_new_cards: boolean
  desired_retention: number
}

interface UpdateSettingsRequest {
  show_percentage?: boolean
  red_threshold?: number
  yellow_threshold?: number
  day_boundary_hour?: number
  auto_progress_on_correct?: boolean
  auto_progress_delay?: number
  suppress_new_cards?: boolean
  desired_retention?: number
}

interface UpdateSettingsResponse {
  success: boolean
}

export async function getUserSettings(): Promise<UserSettings> {
  const url = `${window.location.origin}/api/user/settings`
  return fetchWithAuth(url)
}

export async function updateUserSettings(settings: UpdateSettingsRequest): Promise<UpdateSettingsResponse> {
  const url = `${window.location.origin}/api/user/settings`
  return fetchWithAuth(url, {
    method: 'POST',
    body: JSON.stringify(settings),
  })
}

// Custom Cards API

interface CustomCard {
  id: number
  form: string
  hint: string
  context: string
  context_translation: string
  grammar: string | null
  politeness: string | null
  notes: string[]
  created_at: string
  language_id: number
}

interface CreateCustomCardRequest {
  form: string
  hint: string
  context: string
  context_translation: string
  grammar?: string | null
  politeness?: string | null
  notes?: string[]
}

interface CreateCustomCardResponse {
  id: number
  success: boolean
}

interface ListCustomCardsResponse {
  cards: CustomCard[]
}

interface UpdateCustomCardRequest {
  form?: string
  hint?: string
  context?: string
  context_translation?: string
  grammar?: string | null
  politeness?: string | null
  notes?: string[]
}

interface UpdateCustomCardResponse {
  success: boolean
}

interface DeleteCustomCardResponse {
  success: boolean
}

export async function createCustomCard(card: CreateCustomCardRequest): Promise<CreateCustomCardResponse> {
  const url = `${window.location.origin}/api/custom-cards`
  return fetchWithAuth(url, {
    method: 'POST',
    body: JSON.stringify(card),
  })
}

export async function listCustomCards(): Promise<ListCustomCardsResponse> {
  const url = `${window.location.origin}/api/custom-cards`
  return fetchWithAuth(url)
}

export async function getCustomCard(cardId: number): Promise<CustomCard> {
  const url = `${window.location.origin}/api/custom-cards/${cardId}`
  return fetchWithAuth(url)
}

export async function updateCustomCard(cardId: number, updates: UpdateCustomCardRequest): Promise<UpdateCustomCardResponse> {
  const url = `${window.location.origin}/api/custom-cards/${cardId}`
  return fetchWithAuth(url, {
    method: 'POST',
    body: JSON.stringify(updates),
  })
}

export async function deleteCustomCard(cardId: number): Promise<DeleteCustomCardResponse> {
  const url = `${window.location.origin}/api/custom-cards/${cardId}`
  return fetchWithAuth(url, {
    method: 'DELETE',
  })
}

export { ApiError }
export type { 
  CardResponse, 
  ReviewRequest, 
  ReviewResponse, 
  LanguageInfo, 
  UserProfile, 
  ImportResponse, 
  StatsResponse, 
  UserSettings, 
  UpdateSettingsRequest,
  CustomCard,
  CreateCustomCardRequest,
  CreateCustomCardResponse,
  ListCustomCardsResponse,
  UpdateCustomCardRequest,
  UpdateCustomCardResponse,
  DeleteCustomCardResponse,
  SuppressedCard,
  SuppressedCardsResponse,
}