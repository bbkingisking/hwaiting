import { createContext, useContext, useEffect, useState } from 'react'
import { getFieldValues, type FieldValue } from '@/lib/api'
import { useAuth } from '@/components/auth-provider'

type FieldValueMap = Record<string, FieldValue>

interface FieldValuesContextType {
  pos: FieldValueMap
  originType: FieldValueMap
  grade: FieldValueMap
  speechLevel: FieldValueMap
  tense: FieldValueMap
  grammarPattern: FieldValueMap
}

const emptyFieldValues: FieldValuesContextType = {
  pos: {},
  originType: {},
  grade: {},
  speechLevel: {},
  tense: {},
  grammarPattern: {},
}

function toMap(entries: FieldValue[] | null | undefined): FieldValueMap {
  return Object.fromEntries((entries ?? []).map(e => [e.slug, e]))
}

const FieldValuesContext = createContext<FieldValuesContextType | undefined>(undefined)

export function FieldValuesProvider({ children }: { children: React.ReactNode }) {
  const [fieldValues, setFieldValues] = useState<FieldValuesContextType>(emptyFieldValues)
  const { isAuthenticated } = useAuth()

  useEffect(() => {
    if (!isAuthenticated) {
      return
    }

    const fetchFieldValues = async () => {
      try {
        // No `?fields=` filter: this provider caches the full set once per
        // session for every card-editing/review surface to share.
        const data = await getFieldValues()
        setFieldValues({
          pos: toMap(data.pos),
          originType: toMap(data.origin_type),
          grade: toMap(data.grade),
          speechLevel: toMap(data.speech_level),
          tense: toMap(data.tense),
          grammarPattern: toMap(data.grammar_pattern),
        })
      } catch (err) {
        console.error('Failed to fetch field values:', err)
      }
    }

    fetchFieldValues()
  }, [isAuthenticated])

  return (
    <FieldValuesContext.Provider value={fieldValues}>
      {children}
    </FieldValuesContext.Provider>
  )
}

export function useFieldValues() {
  const context = useContext(FieldValuesContext)
  if (!context) {
    throw new Error('useFieldValues must be used within a FieldValuesProvider')
  }
  return context
}
