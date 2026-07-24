// API module for card operations and user management
//
// Request/response types are generated from the backend's OpenAPI spec
// (see scripts/generate-api-types.sh) rather than hand-typed, so a renamed
// or restructured backend field shows up as a compile error here instead of
// silently drifting (this is exactly the class of bug that caused the old
// card_states_imported/card_states_derived mismatch).

import type { components } from './api-schema'

type Schemas = components['schemas']

// admin::edit_card accepts/returns freeform JSON (serde_json::Value) by
// design - there's no backend schema to generate these from, so they stay
// hand-typed deliberately.
interface EditCardRequest {
  word?: string
  definition?: string | null
  pos?: string | null
  origin_type?: string | null
  hanja?: string | null
  hanja_eum?: string | null
  grade?: string | null
  trans_word?: string
  trans_dfn?: string | null
  sentence?: string
  sentence_translation?: string
  target?: string
  alternatives?: string[]
  speech_level?: string | null
  tense?: string | null
  grammar_pattern?: string | null
}

interface EditCardResponse {
  success: boolean
}

type CardResponse = Schemas['NextCardResponse']
type NextCardEnvelope = Schemas['NextCardEnvelope']
type ReviewRequest = Schemas['ReviewRequest']
type ReviewResponse = Schemas['ReviewResponse']
type UserProfile = Schemas['UserProfile']

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

  if (response.status === 204) {
    return { success: true }
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

export type EnumEntry = Schemas['EnumEntry']
export type EnumLookups = Schemas['EnumLookups']

export async function getEnumLookups(): Promise<EnumLookups> {
  const url = `${window.location.origin}/api/cards/enum-lookups`
  return fetchWithAuth(url)
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

type SuppressedCard = Schemas['SuppressedCard']
type SuppressedCardsResponse = Schemas['SuppressedCardsResponse']

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

type ImportStats = Schemas['ImportStats']
type ImportResponse = Schemas['ImportDataResponse']

export async function importUserData(file: File, overwrite: boolean = false): Promise<ImportResponse> {
  const text = await file.text()
  const data = JSON.parse(text)

  const url = `${window.location.origin}/api/user/import`
  return fetchWithAuth(url, {
    method: 'POST',
    body: JSON.stringify({ data, overwrite }),
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
  link.download = `hwaiting-export-${new Date().toISOString().split('T')[0]}.json`
  document.body.appendChild(link)
  link.click()
  document.body.removeChild(link)
  window.URL.revokeObjectURL(downloadUrl)
}

type StatsResponse = Schemas['StatsResponse']

export type DayHistory = Schemas['DayHistory']
export type ReviewHistoryResponse = Schemas['ReviewHistoryResponse']
export type HistorySummary = Schemas['HistorySummary']

export async function getReviewHistory(): Promise<ReviewHistoryResponse> {
  const url = `${window.location.origin}/api/cards/history`
  return fetchWithAuth(url)
}

export async function getHistorySummary(): Promise<HistorySummary> {
  const url = `${window.location.origin}/api/cards/history-summary`
  return fetchWithAuth(url)
}

export type BreakdownRow = Schemas['BreakdownRow']
export type HistoryBreakdownResponse = Schemas['HistoryBreakdownResponse']

export async function getHistoryBreakdown(): Promise<HistoryBreakdownResponse> {
  const url = `${window.location.origin}/api/cards/history-breakdown`
  return fetchWithAuth(url)
}

export async function getStats(): Promise<StatsResponse> {
  const url = `${window.location.origin}/api/cards/stats`
  return fetchWithAuth(url)
}

type UserSettings = Schemas['UserSettings']
type UpdateSettingsRequest = Schemas['UpdateSettingsRequest']
type UpdateSettingsResponse = Schemas['UpdateSettingsResponse']

export async function getUserSettings(): Promise<UserSettings> {
  const url = `${window.location.origin}/api/user/settings`
  return fetchWithAuth(url)
}

export async function updateUserSettings(settings: UpdateSettingsRequest): Promise<UpdateSettingsResponse> {
  const url = `${window.location.origin}/api/user/settings`
  return fetchWithAuth(url, {
    method: 'PATCH',
    body: JSON.stringify(settings),
  })
}

// Custom Cards API

type CustomCard = Schemas['CustomCard']
type CreateCustomCardRequest = Schemas['CreateCustomCardRequest']
type CreateCustomCardResponse = Schemas['CreateCustomCardResponse']
type ListCustomCardsResponse = Schemas['ListCustomCardsResponse']
type UpdateCustomCardRequest = Schemas['UpdateCustomCardRequest']
type UpdateCustomCardResponse = Schemas['UpdateCustomCardResponse']

// custom_cards::delete_custom_card returns 204 No Content - no response body,
// no backend schema. fetchWithAuth synthesizes { success: true } for any 204.
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

type AdminCard = Schemas['AdminCard']
type SearchCardsResponse = Schemas['SearchCardsResponse']

export async function searchCardsByTarget(query: string, signal?: AbortSignal): Promise<SearchCardsResponse> {
  const url = `${window.location.origin}/api/admin/cards/search?q=${encodeURIComponent(query)}`
  return fetchWithAuth(url, { signal })
}

export async function editCard(cardId: number, updates: EditCardRequest): Promise<EditCardResponse> {
  const url = `${window.location.origin}/api/admin/cards/${cardId}`
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

// FSRS Parameter Optimization

type OptimizeFsrsResponse = Schemas['OptimizeFsrsResponse']

export async function optimizeFsrs(): Promise<OptimizeFsrsResponse> {
  const url = `${window.location.origin}/api/cards/optimize-fsrs`
  return fetchWithAuth(url, {
    method: 'POST',
  })
}

export async function resetFsrsParameters(): Promise<{ success: boolean }> {
  const url = `${window.location.origin}/api/cards/optimize-fsrs`
  return fetchWithAuth(url, {
    method: 'DELETE',
  })
}

export { ApiError }
export type {
  EditCardRequest,
  EditCardResponse,
  CardResponse,
  NextCardEnvelope,
  ReviewRequest,
  ReviewResponse,
  UserProfile,
  ImportResponse,
  ImportStats,
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
  AdminCard,
  SearchCardsResponse,
}
