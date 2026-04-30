import { useState, useEffect } from 'react'
import { useAuth } from '@/components/auth-provider'
import { getUserProfile, type LanguageInfo } from '@/lib/api'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'
import { SettingsDialog } from '@/components/settings-dialog'
import { Settings, Moon, Sun, LogOut } from 'lucide-react'
import { useTheme } from '@/components/theme-provider'

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
  const { username, isAuthenticated, logout } = useAuth()
  const { theme, setTheme } = useTheme()
  const [targetLanguage, setTargetLanguage] = useState<LanguageInfo | null>(null)
  const [settingsOpen, setSettingsOpen] = useState(false)

  useEffect(() => {
    if (isAuthenticated) {
      getUserProfile()
        .then(profile => setTargetLanguage(profile.target_language))
        .catch(err => console.error('Error fetching user profile:', err))
    } else {
      setTargetLanguage(null)
    }
  }, [isAuthenticated])

  const toggleTheme = () => {
    setTheme(theme === 'dark' ? 'light' : 'dark')
  }

  if (!isAuthenticated || !username) {
    return null
  }

  return (
    <>
      <div className="fixed top-4 right-4 z-50 flex items-center gap-3">
        {targetLanguage && (
          <div className="px-2 py-1.5 rounded-md bg-background/80 backdrop-blur-sm border border-border">
            <span className={`fi ${languageFlags[targetLanguage.code] || 'fi-xx'} text-lg`}></span>
          </div>
        )}
        
        <DropdownMenu>
          <DropdownMenuTrigger className="px-3 py-1.5 rounded-md bg-background/80 backdrop-blur-sm border border-border hover:bg-accent hover:text-accent-foreground transition-colors outline-none focus-visible:ring-2 focus-visible:ring-ring">
            <span className="text-sm font-medium">{username}</span>
          </DropdownMenuTrigger>
          <DropdownMenuContent align="end" className="w-48">
            <DropdownMenuItem onClick={() => setSettingsOpen(true)}>
              <Settings className="mr-2 h-4 w-4" />
              Settings
            </DropdownMenuItem>
            <DropdownMenuItem onClick={toggleTheme}>
              {theme === 'dark' ? (
                <>
                  <Sun className="mr-2 h-4 w-4" />
                  Light Mode
                </>
              ) : (
                <>
                  <Moon className="mr-2 h-4 w-4" />
                  Dark Mode
                </>
              )}
            </DropdownMenuItem>
            <DropdownMenuSeparator />
            <DropdownMenuItem onClick={logout}>
              <LogOut className="mr-2 h-4 w-4" />
              Log Out
            </DropdownMenuItem>
          </DropdownMenuContent>
        </DropdownMenu>
      </div>

      <SettingsDialog open={settingsOpen} onOpenChange={setSettingsOpen} />
    </>
  )
}