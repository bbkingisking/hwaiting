// API module for card operations and user management

interface CardResponse {
  card_id: number
  word: string
  definition: string | null
  pos: string | null
  origin_type: string | null
  hanja: string | null
  hanja_eum: string | null
  grade: string | null
  trans_word: string
  trans_dfn: string | null
  sentence: string
  sentence_translation: string
  target: string
  speech_level: string | null
  tense: string | null
  difficulty: number | null
  guess_count: number
  wrong_guess_count: number
}

interface NextCardEnvelope {
  card: CardResponse | null
  next_due_at: string | null
}

interface ReviewRequest {
  rating: number // 1 = Again, 3 = Good
}

interface ReviewResponse {
  success: boolean
}

interface UserProfile {
  username: string
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
  excludeCardId?: number
  signal?: AbortSignal
}

export async function getNextCard(options: GetNextCardOptions = {}): Promise<NextCardEnvelope> {
  const params = new URLSearchParams()
  if (options.excludeCardId !== undefined) {
    params.set('exclude', String(options.excludeCardId))
  }
  const qs = params.toString()
  const url = `${window.location.origin}/api/cards/next${qs ? `?${qs}` : ''}`
  return fetchWithAuth(url, { signal: options.signal })
}

export async function submitReview(cardId: number, rating: number): Promise<ReviewResponse> {
  const url = `${window.location.origin}/api/cards/${cardId}/review`
  return fetchWithAuth(url, {
    method: 'POST',
    body: JSON.stringify({ rating }),
  })
}

export async function suppressCard(cardId: number): Promise<ReviewResponse> {
  const url = `${window.location.origin}/api/cards/${cardId}/suppress`
  return fetchWithAuth(url, {
    method: 'PUT',
  })
}

interface SuppressedCard {
  card_id: number
  word: string
  trans_word: string
  sentence: string
  sentence_translation: string
  pos: string | null
  grade: string | null
}

interface SuppressedCardsResponse {
  cards: SuppressedCard[]
}

export async function listSuppressedCards(): Promise<SuppressedCardsResponse> {
  const url = `${window.location.origin}/api/cards/suppressed`
  return fetchWithAuth(url)
}

export async function unsuppressCard(cardId: number): Promise<ReviewResponse> {
  const url = `${window.location.origin}/api/cards/${cardId}/unsuppress`
  return fetchWithAuth(url, {
    method: 'PUT',
  })
}

export async function getUserProfile(): Promise<UserProfile> {
  const url = `${window.location.origin}/api/user/me`
  return fetchWithAuth(url)
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
  new_today_count: number
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
  desired_retention: number
  daily_new_card_limit: number
}

interface UpdateSettingsRequest {
  show_percentage?: boolean
  red_threshold?: number
  yellow_threshold?: number
  day_boundary_hour?: number
  auto_progress_on_correct?: boolean
  auto_progress_delay?: number
  desired_retention?: number
  daily_new_card_limit?: number
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
  word: string
  definition: string | null
  pos: string | null
  grade: string | null
  origin_type: string | null
  hanja: string | null
  hanja_eum: string | null
  trans_word: string
  trans_dfn: string | null
  sentence: string
  sentence_translation: string
  target: string
  speech_level: string | null
  tense: string | null
  created_at: string
}

interface CreateCustomCardRequest {
  word: string
  definition?: string | null
  pos?: string | null
  grade?: string | null
  origin_type?: string | null
  hanja?: string | null
  hanja_eum?: string | null
  trans_word: string
  trans_dfn?: string | null
  sentence: string
  sentence_translation: string
  target: string
  speech_level?: string | null
  tense?: string | null
}

interface CreateCustomCardResponse {
  id: number
  success: boolean
}

interface ListCustomCardsResponse {
  cards: CustomCard[]
}

interface UpdateCustomCardRequest {
  word?: string
  definition?: string | null
  pos?: string | null
  grade?: string | null
  origin_type?: string | null
  hanja?: string | null
  hanja_eum?: string | null
  trans_word?: string
  trans_dfn?: string | null
  sentence?: string
  sentence_translation?: string
  target?: string
  speech_level?: string | null
  tense?: string | null
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
    method: 'PATCH',
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
  NextCardEnvelope,
  ReviewRequest,
  ReviewResponse,
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