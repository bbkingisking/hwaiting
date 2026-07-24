import { createContext, useContext, useEffect, useState, type ReactNode } from 'react'
import { CARD_THEMES, DEFAULT_CARD_THEME, getCardTheme, type CardThemeId } from '@/lib/card-themes'
import { STORAGE_KEYS } from '@/lib/constants'

interface CardThemeContextValue {
  cardThemeId: CardThemeId
  setCardThemeId: (id: CardThemeId) => void
}

const CardThemeContext = createContext<CardThemeContextValue | null>(null)

const FONT_LINK_ID = 'card-theme-font-link'

export function CardThemeProvider({ children }: { children: ReactNode }) {
  const [cardThemeId, setCardThemeIdState] = useState<CardThemeId>(() => {
    if (typeof window === 'undefined') return DEFAULT_CARD_THEME
    const stored = localStorage.getItem(STORAGE_KEYS.CARD_THEME) as CardThemeId | null
    return stored && CARD_THEMES.some((t) => t.id === stored) ? stored : DEFAULT_CARD_THEME
  })

  useEffect(() => {
    localStorage.setItem(STORAGE_KEYS.CARD_THEME, cardThemeId)

    const theme = getCardTheme(cardThemeId)
    let link = document.getElementById(FONT_LINK_ID) as HTMLLinkElement | null

    if (!theme.fontUrl) {
      link?.remove()
      return
    }

    if (!link) {
      link = document.createElement('link')
      link.id = FONT_LINK_ID
      link.rel = 'stylesheet'
      document.head.appendChild(link)
    }
    link.href = theme.fontUrl
  }, [cardThemeId])

  const setCardThemeId = (id: CardThemeId) => setCardThemeIdState(id)

  return (
    <CardThemeContext.Provider value={{ cardThemeId, setCardThemeId }}>
      {children}
    </CardThemeContext.Provider>
  )
}

export function useCardTheme() {
  const ctx = useContext(CardThemeContext)
  if (!ctx) throw new Error('useCardTheme must be used within a CardThemeProvider')
  return { ...ctx, theme: getCardTheme(ctx.cardThemeId) }
}
