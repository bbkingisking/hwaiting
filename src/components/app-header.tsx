import { useState, useEffect } from 'react'
import { ModeToggle } from '@/components/mode-toggle'
import { SettingsDialog } from '@/components/settings-dialog'
import { useAuth } from '@/components/auth-provider'
import { getUserProfile, type LanguageInfo } from '@/lib/api'

// Map language codes to flag-icons classes
const languageFlags: Record<string, string> = {
  ko: 'fi-kr',
  ja: 'fi-jp',
  es: 'fi-es',
  fr: 'fi-fr',
  de: 'fi-de',
  zh: 'fi-cn',
}

export function AppHeader() {
  const { username, isAuthenticated } = useAuth()
  const [targetLanguage, setTargetLanguage] = useState<LanguageInfo | null>(null)

  useEffect(() => {
    if (isAuthenticated) {
      getUserProfile()
        .then(profile => setTargetLanguage(profile.target_language))
        .catch(err => console.error('Error fetching user profile:', err))
    } else {
      setTargetLanguage(null)
    }
  }, [isAuthenticated])

  return (
    <div className="fixed top-4 right-4 z-50 flex items-center gap-2">
      {username && (
        <div className="px-3 py-1.5 rounded-md bg-background/80 backdrop-blur-sm border border-border">
          <span className="text-sm font-medium text-foreground">{username}</span>
        </div>
      )}
      {username && targetLanguage && (
        <div className="px-2 py-1.5 rounded-md bg-background/80 backdrop-blur-sm border border-border">
          <span className={`fi ${languageFlags[targetLanguage.code] || 'fi-xx'} text-lg`}></span>
        </div>
      )}
      <SettingsDialog />
      <ModeToggle />
    </div>
  )
}