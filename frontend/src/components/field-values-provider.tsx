import { createContext, useContext, useEffect, useState } from 'react'
import { getFieldValues, type FieldValue, type InflectionFormValue } from '@/lib/api'
import { useAuth } from '@/components/auth-provider'

type FieldValueMap = Record<string, FieldValue>
type InflectionFormMap = Record<string, InflectionFormValue>

interface FieldValuesContextType {
  pos: FieldValueMap
  originType: FieldValueMap
  grade: FieldValueMap
  speechLevel: FieldValueMap
  tense: FieldValueMap
  grammarPattern: FieldValueMap
  // Catalog of possible inflected forms (label/ending/category), keyed by
  // slug - non-spoiling, unlike the actual conjugated forms for one card
  // (CardReveal.inflections), which only carry the same slug plus the form
  // itself and are looked up against this map. See InflectionsDialog.
  inflectionForm: InflectionFormMap
}

const emptyFieldValues: FieldValuesContextType = {
  pos: {},
  originType: {},
  grade: {},
  speechLevel: {},
  tense: {},
  grammarPattern: {},
  inflectionForm: {},
}

function toMap<T extends { slug: string }>(entries: T[] | null | undefined): Record<string, T> {
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
          inflectionForm: toMap(data.inflection_form),
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
