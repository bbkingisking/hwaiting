import { subscribeLiveEvents } from './live-events'
// API module for card operations and user management
//
// Request/response types are generated from the backend's OpenAPI spec
// (see scripts/generate-api-types.sh) rather than hand-typed, so a renamed
// or restructured backend field shows up as a compile error here instead of
// silently drifting (this is exactly the class of bug that caused the old
// card_states_imported/card_states_derived mismatch).

import type * as Schemas from './api-generated/types.gen'
import { apiBase, attestationToJson, describeCeremonyError, toCreationOptions } from './webauthn'
import { fetchWithCookies } from './http'

type CheckRequest = Schemas.CheckRequest
type CheckResponse = Schemas.CheckResponse
type ReviewResponse = Schemas.ReviewResponse
type UserProfile = Schemas.UserProfile

export type ReviewQueueChange = 'availability' | 'import'
const reviewQueueChanges = new EventTarget()
let reviewQueueRevision = 0
subscribeLiveEvents(event => { if (!event.own) reviewQueueRevision++ })

// Review submissions and suppression already refresh through App's handlers.
// Mutations elsewhere publish here, after the server has committed them.
function notifyReviewQueueChanged(change: ReviewQueueChange) {
  reviewQueueRevision++
  reviewQueueChanges.dispatchEvent(new CustomEvent('change', { detail: change }))
}

export function subscribeReviewQueueChanges(listener: (change: ReviewQueueChange) => void): () => void {
  const onChange = (event: Event) => listener((event as CustomEvent<ReviewQueueChange>).detail)
  reviewQueueChanges.addEventListener('change', onChange)
  return () => reviewQueueChanges.removeEventListener('change', onChange)
}

class ApiError extends Error {
  constructor(public status: number, message: string, public code?: Schemas.ErrorCode) {
    super(message)
    this.name = 'ApiError'
  }
}

async function fetchWithAuth(url: string, options: RequestInit = {}) {
  const response = await fetchWithCookies(url, {
    ...options,
    headers: {
      ...options.headers,
      'Content-Type': 'application/json',
    },
    signal: options.signal,
  })

  if (!response.ok) {
    const error = await response.json().catch(() => ({ error: 'Unknown error' }))
    throw new ApiError(response.status, error.error || `HTTP ${response.status}`, error.code)
  }

  if (response.status === 204) {
    return { success: true }
  }

  return response.json()
}

type Capabilities = Schemas.Capabilities

// Public: AuthProvider checks this before sign-in to decide whether to
// offer passkeys. An existing authentication cookie is optional.
export async function getCapabilities(): Promise<Capabilities> {
  const response = await fetchWithCookies(`${apiBase()}/api/capabilities`)
  if (!response.ok) {
    throw new ApiError(response.status, `HTTP ${response.status}`)
  }
  return response.json()
}

export type ReviewQueueResponse = Schemas.ReviewQueueResponse

// The browser buffers one successor and refreshes after each answer.
// Review content requests send the account’s saved translation language explicitly.
// Admin browsing and editing choose their translation language independently.
export async function getAvailableTranslationLanguages(signal?: AbortSignal): Promise<Schemas.AvailableTranslationLanguage[]> {
  const response: Schemas.AvailableTranslationLanguagesResponse = await fetchWithAuth(`${apiBase()}/api/cards/languages`, { signal })
  return response.languages
}

export async function getReviewQueue(signal?: AbortSignal): Promise<ReviewQueueResponse> {
  while (true) {
    const revision = reviewQueueRevision
    const result = await fetchWithAuth(`${apiBase()}/api/reviews/queue?limit=2`, { signal })
    if (revision === reviewQueueRevision) return result
  }
}

export type CardInflection = Schemas.CardInflection
export async function getGrammarPatterns(signal?: AbortSignal): Promise<Schemas.GrammarPatternOption[]> {
  const response: Schemas.GrammarPatternsResponse = await fetchWithAuth(`${apiBase()}/api/resources/grammar-patterns`, { signal })
  return response.grammar_patterns
}

// Grades `answer` against the token’s prompt server-side, records the FSRS review,
// and returns the card's secret half (target, word, hanja reading/gloss,
// grammar pattern endings) - none of which the client had before this call.
export async function checkAnswer(attemptToken: string, answer: string): Promise<CheckResponse> {
  const url = `${apiBase()}/api/reviews/answer`
  const body: CheckRequest = { answer, attempt_token: attemptToken }
  return fetchWithAuth(url, {
    method: 'POST',
    body: JSON.stringify(body),
  })
}

// A successful validation is a snapshot, not a reservation; grading validates again.
export async function validateAttempt(attemptToken: string): Promise<boolean> {
  const body: Schemas.ValidateRequest = { attempt_token: attemptToken }
  const result: Schemas.ValidateResponse = await fetchWithAuth(`${apiBase()}/api/reviews/check-freshness`, {
    method: 'POST',
    body: JSON.stringify(body),
  })
  return result.valid
}

export async function suppressCard(cardId: number): Promise<ReviewResponse> {
  const url = `${apiBase()}/api/cards/${cardId}/suppress`
  return fetchWithAuth(url, {
    method: 'PUT',
  })
}

type SuppressedCard = Schemas.SuppressedCard
type SuppressedCardsResponse = Schemas.SuppressedCardsResponse

export async function listSuppressedCards(signal?: AbortSignal): Promise<SuppressedCardsResponse> {
  const url = `${apiBase()}/api/cards/suppressed`
  return fetchWithAuth(url, { signal })
}

export async function unsuppressCard(cardId: number): Promise<ReviewResponse> {
  const url = `${apiBase()}/api/cards/${cardId}/unsuppress`
  const result = await fetchWithAuth(url, {
    method: 'PUT',
  })
  notifyReviewQueueChanged('availability')
  return result
}

type HanjaDrill = Schemas.HanjaDrill
type HanjaDrillResponse = Schemas.HanjaDrillResponse

// Not part of the review flow: a random hanja from a card the user has
// already mastered, for free-form recall practice. Never grades the answer
// and never touches FSRS state - see backend cards/hanja_drill.rs.
export async function getHanjaDrill(signal?: AbortSignal): Promise<HanjaDrillResponse> {
  const url = `${apiBase()}/api/drills/hanja`
  return fetchWithAuth(url, { signal })
}

export async function getUserProfile(): Promise<UserProfile> {
  const url = `${apiBase()}/api/user/me`
  return fetchWithAuth(url)
}

export async function logoutSession(): Promise<void> {
  await fetchWithAuth(`${apiBase()}/api/auth/logout`, { method: 'POST' })
}

type PasskeySummary = Schemas.PasskeySummary
type ListPasskeysResponse = Schemas.ListPasskeysResponse

export async function listPasskeys(): Promise<ListPasskeysResponse> {
  const url = `${apiBase()}/api/user/passkeys`
  return fetchWithAuth(url)
}

export async function deletePasskey(id: number): Promise<{ success: boolean }> {
  const url = `${apiBase()}/api/user/passkeys/${id}`
  return fetchWithAuth(url, { method: 'DELETE' })
}

export async function updatePasskeyLabel(id: number, label: string): Promise<PasskeySummary> {
  return fetchWithAuth(`${apiBase()}/api/user/passkeys/${id}`, {
    method: 'PATCH',
    body: JSON.stringify({ label } satisfies Schemas.UpdatePasskeyRequest),
  })
}

// Registers a new passkey and attaches it to the signed-in account - the
// authenticated counterpart to AuthProvider's public `register()`, which
// creates the account itself. Runs the full ceremony (start -> browser ->
// finish) in one call since callers never need the ceremony id mid-flight.
export async function addPasskey(): Promise<PasskeySummary> {
  if (!window.PublicKeyCredential) {
    throw new Error('This browser does not support passkeys.')
  }

  const { ceremony_id, options } = await fetchWithAuth(`${apiBase()}/api/user/passkeys/register/start`, {
    method: 'POST',
  })

  let attestation: PublicKeyCredential
  try {
    attestation = (await navigator.credentials.create({
      publicKey: toCreationOptions(options),
    })) as PublicKeyCredential
  } catch (e) {
    throw new Error(describeCeremonyError(e, 'Adding a passkey'))
  }

  return fetchWithAuth(`${apiBase()}/api/user/passkeys/register/finish`, {
    method: 'POST',
    body: JSON.stringify({ ceremony_id, credential: attestationToJson(attestation) }),
  })
}

type ImportResponse = Schemas.ImportDataResponse

export async function importUserData(file: File, overwrite: boolean = false): Promise<ImportResponse> {
  const text = await file.text()
  const data = JSON.parse(text)

  const url = `${apiBase()}/api/user/import`
  const result = await fetchWithAuth(url, {
    method: 'POST',
    body: JSON.stringify({ data, overwrite }),
  })
  notifyReviewQueueChanged('import')
  return result
}

export async function exportUserData(): Promise<void> {
  const url = `${apiBase()}/api/user/export`
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

type TodayStatsResponse = Schemas.TodayStatsResponse

export type DayHistory = Schemas.DayHistory
export type HistorySummary = Schemas.HistorySummary
export type BreakdownRow = Schemas.BreakdownRow
type HistoryResponse = Schemas.HistoryResponse

// Was three separate round-trips (getReviewHistory/getHistorySummary/
// getHistoryBreakdown) for data the stats page always fetched together;
// the backend now runs all three queries concurrently behind one endpoint.
export async function getHistory(): Promise<HistoryResponse> {
  const url = `${apiBase()}/api/stats/history`
  return fetchWithAuth(url)
}

export async function getTodayStats(): Promise<TodayStatsResponse> {
  const url = `${apiBase()}/api/stats/today`
  return fetchWithAuth(url)
}

type UserSettings = Schemas.UserSettings
type UpdateSettingsRequest = Schemas.UpdateSettingsRequest

export async function getUserSettings(): Promise<UserSettings> {
  const url = `${apiBase()}/api/user/settings`
  let settings: UserSettings = await fetchWithAuth(url)
  if (settings.study_timezone == null) {
    const timezone = Intl.DateTimeFormat().resolvedOptions().timeZone || 'UTC'
    // A different device may have initialized it first; the response is the saved value.
    settings = await updateUserSettings({ initial_study_timezone: timezone })
  }
  return settings
}

export async function updateUserSettings(settings: UpdateSettingsRequest, signal?: AbortSignal): Promise<UserSettings> {
  const url = `${apiBase()}/api/user/settings`
  const result = await fetchWithAuth(url, {
    method: 'PATCH',
    body: JSON.stringify(settings),
    signal,
  })
  if (settings.translation_language != null || settings.show_hanja_hints != null || settings.study_timezone != null || settings.initial_study_timezone != null || settings.daily_new_card_limit != null || settings.day_boundary_hour != null || settings.desired_retention != null) {
    notifyReviewQueueChanged('availability')
  }
  return result
}

// Flat card projection is retained for review JSON copying; the editor loads AdminCardDetail.
type AdminCard = Schemas.CardFront & Schemas.CardBack
export type AdminCardDetail = Schemas.AdminCardDetail
export type AdminCardSummary = Schemas.AdminCardSummary
export type CardEdit = { card: AdminCardDetail }
const cardEdits = new EventTarget()
export function subscribeCardEdits(listener: (edit: CardEdit) => void): () => void {
  const onEdit = (event: Event) => listener((event as CustomEvent<CardEdit>).detail)
  cardEdits.addEventListener('edit', onEdit)
  return () => cardEdits.removeEventListener('edit', onEdit)
}

type SearchCardsResponse = Schemas.AdminSearchResponse

export async function searchCardsByTarget(query: string, signal?: AbortSignal): Promise<SearchCardsResponse> {
  const url = `${apiBase()}/api/admin/cards/search?q=${encodeURIComponent(query)}`
  return fetchWithAuth(url, { signal })
}

export async function getAdminCard(cardId: number, signal?: AbortSignal): Promise<AdminCardDetail> {
  return fetchWithAuth(`${apiBase()}/api/admin/cards/${cardId}`, { signal })
}

export async function saveAdminCard(cardId: number, updates: Schemas.SaveAdminCardRequest): Promise<Schemas.SaveAdminCardResponse> {
  const result: Schemas.SaveAdminCardResponse = await fetchWithAuth(`${apiBase()}/api/admin/cards/${cardId}`, {
    method: 'PATCH', body: JSON.stringify(updates),
  })
  cardEdits.dispatchEvent(new CustomEvent<CardEdit>('edit', { detail: { card: result.card } }))
  return result
}

type CardInflectionsResponse = Schemas.CardInflectionsResponse

// This card's resolved conjugation-matrix rows, fetched standalone rather
// than via checkAnswer's CardReveal - admin browsing already discloses
// target/word/sentence up front (see SearchCardsResponse), so there's no
// review-flow secrecy left to preserve here.
export async function getCardInflections(cardId: number, signal?: AbortSignal): Promise<CardInflectionsResponse> {
  const url = `${apiBase()}/api/admin/cards/${cardId}/inflections`
  return fetchWithAuth(url, { signal })
}

// FSRS Parameter Optimization

type OptimizeFsrsResponse = Schemas.OptimizeFsrsResponse

export async function optimizeFsrs(): Promise<OptimizeFsrsResponse> {
  const url = `${apiBase()}/api/scheduling/fsrs-parameters`
  const result = await fetchWithAuth(url, {
    method: 'POST',
  })
  notifyReviewQueueChanged('availability')
  return result
}

export async function resetFsrsParameters(): Promise<{ success: boolean }> {
  const url = `${apiBase()}/api/scheduling/fsrs-parameters`
  const result = await fetchWithAuth(url, {
    method: 'DELETE',
  })
  notifyReviewQueueChanged('availability')
  return result
}

export { ApiError }
export type {
  CheckResponse,
  ImportResponse,
  SuppressedCard,
  HanjaDrill,
  AdminCard,
  PasskeySummary,
}

export type FeedbackEntry = Schemas.FeedbackEntry

export async function submitFeedback(sentenceId: number, text: string): Promise<Schemas.FeedbackResponse> {
  const body: Schemas.FeedbackRequest = { body: text }
  return fetchWithAuth(`${apiBase()}/api/feedback/submit?sentence_id=${sentenceId}`, {
    method: 'POST',
    body: JSON.stringify(body),
  })
}

export async function listFeedback(signal?: AbortSignal): Promise<Schemas.ListFeedbackResponse> {
  return fetchWithAuth(`${apiBase()}/api/admin/feedback`, { signal })
}
