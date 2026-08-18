// API module for card operations and user management
//
// Request/response types are generated from the backend's OpenAPI spec
// (see scripts/generate-api-types.sh) rather than hand-typed, so a renamed
// or restructured backend field shows up as a compile error here instead of
// silently drifting (this is exactly the class of bug that caused the old
// card_states_imported/card_states_derived mismatch).

import type { components } from './api-schema'

type Schemas = components['schemas']

type EditCardRequest = Schemas['UpdateCardRequest']
type EditCardResponse = Schemas['EditCardResponse']

type CardResponse = Schemas['NextCardResponse']
type NextCardEnvelope = Schemas['NextCardEnvelope']
type CheckRequest = Schemas['CheckRequest']
type CheckResponse = Schemas['CheckResponse']
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
  // Card ids the caller doesn't want back - see NextCardQuery::exclude on
  // the backend. Comma-joined into one `exclude` param, since the server's
  // query deserializer has no support for repeated-key arrays.
  excludeCardIds?: number[]
  signal?: AbortSignal
}

export async function getNextCard(options: GetNextCardOptions = {}): Promise<NextCardEnvelope> {
  const params = new URLSearchParams()
  if (options.excludeCardIds?.length) {
    params.set('exclude', options.excludeCardIds.join(','))
  }
  const qs = params.toString()
  const url = `${window.location.origin}/api/cards/next${qs ? `?${qs}` : ''}`
  return fetchWithAuth(url, { signal: options.signal })
}

export type FieldValue = Schemas['FieldValue']
export type FieldValues = Schemas['FieldValues']
export type InflectionFormValue = Schemas['InflectionFormValue']
export type CardInflection = Schemas['CardInflection']

export async function getFieldValues(): Promise<FieldValues> {
  const url = `${window.location.origin}/api/cards/field-values`
  return fetchWithAuth(url)
}

// Grades `answer` against `cardId` server-side, records the FSRS review,
// and returns the card's secret half (target, word, hanja reading/gloss,
// grammar pattern endings) - none of which the client had before this call.
export async function checkAnswer(cardId: number, answer: string): Promise<CheckResponse> {
  const url = `${window.location.origin}/api/cards/${cardId}/check`
  const body: CheckRequest = { answer }
  return fetchWithAuth(url, {
    method: 'POST',
    body: JSON.stringify(body),
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
export type HistorySummary = Schemas['HistorySummary']
export type BreakdownRow = Schemas['BreakdownRow']
export type HistoryBreakdownResponse = Schemas['HistoryBreakdownResponse']
export type HistoryResponse = Schemas['HistoryResponse']

// Was three separate round-trips (getReviewHistory/getHistorySummary/
// getHistoryBreakdown) for data the stats page always fetched together;
// the backend now runs all three queries concurrently behind one endpoint.
export async function getHistory(): Promise<HistoryResponse> {
  const url = `${window.location.origin}/api/cards/history`
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

// Backend's admin::AdminCard and cards::NextCardResponse used to be two
// independently hand-declared structs that happened to agree on most
// fields - now unified into one schema, cards::Card, reused by both.
//
// `Card` is also the shape EditCardDialog edits, review flow included: the
// review card only ever holds the CardPrompt/CardReveal split (see
// lib/types.ts), which withholds `target` et al. pre-answer on purpose, so
// opening the editor there requires an adapter (toAdminCard in
// flashcard.tsx) rather than passing the review card straight through.
type AdminCard = Schemas['Card']

// The subset of AdminCard's fields that CardReveal actually has - i.e. the
// half of a `Card` row the server withholds from `GET /api/cards/next` and
// only discloses once `POST /api/cards/{id}/check` grades an attempt (see
// CardPrompt/CardReveal in lib/types.ts). Every other AdminCard field is
// CardPrompt's. This is the same split toAdminCard (flashcard.tsx) merges
// back together, kept here as the single source of truth both that adapter
// and EditCardDialog's "front"/"back" JSON views read from, rather than each
// hand-maintaining its own copy that could silently drift from the other.
export const CARD_BACK_FIELDS = new Set(['word', 'definition', 'sentence', 'target', 'alternatives'])
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
  const url = `${window.location.origin}/api/cards/fsrs-parameters`
  return fetchWithAuth(url, {
    method: 'POST',
  })
}

export async function resetFsrsParameters(): Promise<{ success: boolean }> {
  const url = `${window.location.origin}/api/cards/fsrs-parameters`
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
  CheckRequest,
  CheckResponse,
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
