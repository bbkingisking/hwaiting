import { createContext, useContext, useEffect, useState } from 'react'
import { getGrammarPatterns, type GrammarPattern } from '@/lib/api'
import { useAuth } from '@/components/auth-provider'

interface GrammarPatternsContextType {
  patterns: Record<string, GrammarPattern>
}

const GrammarPatternsContext = createContext<GrammarPatternsContextType | undefined>(undefined)

export function GrammarPatternsProvider({ children }: { children: React.ReactNode }) {
  const [patterns, setPatterns] = useState<Record<string, GrammarPattern>>({})
  const { isAuthenticated } = useAuth()

  useEffect(() => {
    if (!isAuthenticated) {
      return
    }

    const fetchPatterns = async () => {
      try {
        const list = await getGrammarPatterns()
        setPatterns(Object.fromEntries(list.map(p => [p.slug, p])))
      } catch (err) {
        console.error('Failed to fetch grammar patterns:', err)
      }
    }

    fetchPatterns()
  }, [isAuthenticated])

  return (
    <GrammarPatternsContext.Provider value={{ patterns }}>
      {children}
    </GrammarPatternsContext.Provider>
  )
}

export function useGrammarPatterns() {
  const context = useContext(GrammarPatternsContext)
  if (!context) {
    throw new Error('useGrammarPatterns must be used within a GrammarPatternsProvider')
  }
  return context
}
