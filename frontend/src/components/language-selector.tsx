import { useState, useEffect } from 'react'
import { getLanguages, setUserLanguage, type LanguageInfo } from '@/lib/api'
import { Button } from '@/components/ui/button'
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from '@/components/ui/card'



interface LanguageSelectorProps {
  onLanguageSelected: () => void
}

export function LanguageSelector({ onLanguageSelected }: LanguageSelectorProps) {
  const [languages, setLanguages] = useState<LanguageInfo[]>([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)
  const [submitting, setSubmitting] = useState(false)

  useEffect(() => {
    const fetchLanguages = async () => {
      try {
        const langs = await getLanguages()
        setLanguages(langs)
      } catch (err) {
        setError('Failed to load languages')
        console.error('Error fetching languages:', err)
      } finally {
        setLoading(false)
      }
    }

    fetchLanguages()
  }, [])

  const handleSelectLanguage = async (languageId: number) => {
    setSubmitting(true)
    setError(null)
    try {
      await setUserLanguage(languageId)
      onLanguageSelected()
    } catch (err) {
      setError('Failed to set language')
      console.error('Error setting language:', err)
      setSubmitting(false)
    }
  }

  if (loading) {
    return (
      <div className="min-h-screen flex items-center justify-center">
        <Card className="w-full max-w-md">
          <CardContent className="pt-6">
            <p className="text-center text-muted-foreground">Loading languages...</p>
          </CardContent>
        </Card>
      </div>
    )
  }

  if (error && languages.length === 0) {
    return (
      <div className="min-h-screen flex items-center justify-center">
        <Card className="w-full max-w-md">
          <CardContent className="pt-6">
            <p className="text-center text-destructive">{error}</p>
          </CardContent>
        </Card>
      </div>
    )
  }

  return (
    <div className="min-h-screen flex items-center justify-center p-6">
      <Card className="w-full max-w-md">
        <CardHeader>
          <CardTitle>Choose Your Language</CardTitle>
          <CardDescription>
            Select the language you want to learn
          </CardDescription>
        </CardHeader>
        <CardContent className="space-y-3">
          {error && (
            <p className="text-sm text-destructive mb-2">{error}</p>
          )}
          {languages.map((language) => (
            <Button
              key={language.id}
              variant="outline"
              className="w-full justify-start gap-3"
              size="lg"
              onClick={() => handleSelectLanguage(language.id)}
              disabled={submitting}
            >
              {language.icon && (
                <img src={`/${language.icon}`} alt={`${language.name} flag`} className="w-6 h-4" />
              )}
              <span>{language.name}</span>
            </Button>
          ))}
        </CardContent>
      </Card>
    </div>
  )
}