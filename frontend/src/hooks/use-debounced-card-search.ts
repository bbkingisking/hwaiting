import { useEffect, useState } from 'react'
import { searchCardsByTarget, ApiError, type AdminCard } from '@/lib/api'

interface DebouncedCardSearch {
  query: string
  setQuery: (query: string) => void
  cards: AdminCard[]
  setCards: React.Dispatch<React.SetStateAction<AdminCard[]>>
  isLoading: boolean
  hasSearched: boolean
  error: string | null
  setError: (error: string | null) => void
}

// Shared by BrowseCardsDialog and ConjugationTablesDialog - previously
// hand-copied in both, byte-for-byte down to the comments. Search cards by
// target or card id, debounced by 300ms and cancelled if the query changes
// again before it fires; resets to empty whenever the dialog opens, so a
// stale search from the last time it was open never shows.
export function useDebouncedCardSearch(open: boolean): DebouncedCardSearch {
  const [query, setQuery] = useState('')
  const [cards, setCards] = useState<AdminCard[]>([])
  const [isLoading, setIsLoading] = useState(false)
  const [hasSearched, setHasSearched] = useState(false)
  const [error, setError] = useState<string | null>(null)

  // Reset state when the dialog opens
  useEffect(() => {
    if (open) {
      setQuery('')
      setCards([])
      setError(null)
      setHasSearched(false)
    }
  }, [open])

  // Debounced search on query change
  useEffect(() => {
    if (!open) return

    const trimmed = query.trim()
    if (trimmed === '') {
      setCards([])
      setHasSearched(false)
      setIsLoading(false)
      setError(null)
      return
    }

    // Set inside the timer, not before it: flagging "loading" during the
    // debounce window advertises a request that hasn't been made yet.
    const controller = new AbortController()
    const timer = setTimeout(() => {
      setIsLoading(true)
      searchCardsByTarget(trimmed, controller.signal)
        .then(response => {
          setCards(response.cards)
          setHasSearched(true)
          setError(null)
          setIsLoading(false)
        })
        .catch(err => {
          if (controller.signal.aborted) return
          setError(err instanceof ApiError ? err.message : 'Search failed')
          setIsLoading(false)
        })
    }, 300)

    return () => {
      clearTimeout(timer)
      controller.abort()
    }
  }, [open, query])

  return { query, setQuery, cards, setCards, isLoading, hasSearched, error, setError }
}
