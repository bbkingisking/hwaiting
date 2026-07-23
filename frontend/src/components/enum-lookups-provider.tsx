import { createContext, useContext, useEffect, useState } from 'react'
import { getEnumLookups, type EnumEntry } from '@/lib/api'
import { useAuth } from '@/components/auth-provider'

type EnumMap = Record<string, EnumEntry>

interface EnumLookupsContextType {
  pos: EnumMap
  originType: EnumMap
  grade: EnumMap
  speechLevel: EnumMap
  tense: EnumMap
  grammarPattern: EnumMap
}

const emptyLookups: EnumLookupsContextType = {
  pos: {},
  originType: {},
  grade: {},
  speechLevel: {},
  tense: {},
  grammarPattern: {},
}

function toMap(entries: EnumEntry[]): EnumMap {
  return Object.fromEntries(entries.map(e => [e.slug, e]))
}

const EnumLookupsContext = createContext<EnumLookupsContextType | undefined>(undefined)

export function EnumLookupsProvider({ children }: { children: React.ReactNode }) {
  const [lookups, setLookups] = useState<EnumLookupsContextType>(emptyLookups)
  const { isAuthenticated } = useAuth()

  useEffect(() => {
    if (!isAuthenticated) {
      return
    }

    const fetchLookups = async () => {
      try {
        const data = await getEnumLookups()
        setLookups({
          pos: toMap(data.pos),
          originType: toMap(data.origin_type),
          grade: toMap(data.grade),
          speechLevel: toMap(data.speech_level),
          tense: toMap(data.tense),
          grammarPattern: toMap(data.grammar_pattern),
        })
      } catch (err) {
        console.error('Failed to fetch enum lookups:', err)
      }
    }

    fetchLookups()
  }, [isAuthenticated])

  return (
    <EnumLookupsContext.Provider value={lookups}>
      {children}
    </EnumLookupsContext.Provider>
  )
}

export function useEnumLookups() {
  const context = useContext(EnumLookupsContext)
  if (!context) {
    throw new Error('useEnumLookups must be used within an EnumLookupsProvider')
  }
  return context
}
