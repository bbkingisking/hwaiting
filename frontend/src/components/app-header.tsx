import { useState, useEffect } from 'react'
import { useAuth } from '@/components/auth-provider'
import { getUserProfile, listSuppressedCards, type LanguageInfo } from '@/lib/api'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'
import { SettingsDialog } from '@/components/settings-dialog'
import { CustomCardsDialog } from '@/components/custom-cards-dialog'
import { SuppressedCardsDialog } from '@/components/suppressed-cards-dialog'
import { Settings, Moon, Sun, LogOut, Plus, EyeOff } from 'lucide-react'
import { useTheme } from '@/components/theme-provider'



export function AppHeader() {
  const { username, isAuthenticated, logout } = useAuth()
  const { theme, setTheme } = useTheme()
  const [targetLanguage, setTargetLanguage] = useState<LanguageInfo | null>(null)
  const [settingsOpen, setSettingsOpen] = useState(false)
  const [customCardsOpen, setCustomCardsOpen] = useState(false)
  const [suppressedCardsOpen, setSuppressedCardsOpen] = useState(false)
  const [hasSuppressedCards, setHasSuppressedCards] = useState(false)

  const checkSuppressedCards = () => {
    listSuppressedCards()
      .then(response => setHasSuppressedCards(response.cards.length > 0))
      .catch(err => console.error('Error checking suppressed cards:', err))
  }

  useEffect(() => {
    if (isAuthenticated) {
      getUserProfile()
        .then(profile => setTargetLanguage(profile.target_language))
        .catch(err => console.error('Error fetching user profile:', err))
      
      checkSuppressedCards()
    } else {
      setTargetLanguage(null)
      setHasSuppressedCards(false)
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
            {targetLanguage.icon && (
              <img src={`/${targetLanguage.icon}`} alt={`${targetLanguage.name} flag`} className="w-6 h-4" />
            )}
          </div>
        )}
        
        <DropdownMenu>
          <DropdownMenuTrigger className="px-3 py-1.5 rounded-md bg-background/80 backdrop-blur-sm border border-border hover:bg-accent hover:text-accent-foreground transition-colors outline-none focus-visible:ring-2 focus-visible:ring-ring">
            <span className="text-sm font-medium">{username}</span>
          </DropdownMenuTrigger>
          <DropdownMenuContent align="end" className="w-48">
            <DropdownMenuItem onClick={() => setCustomCardsOpen(true)}>
              <Plus className="mr-2 h-4 w-4" />
              Custom Cards
            </DropdownMenuItem>
            {hasSuppressedCards && (
              <DropdownMenuItem onClick={() => setSuppressedCardsOpen(true)}>
                <EyeOff className="mr-2 h-4 w-4" />
                Suppressed Cards
              </DropdownMenuItem>
            )}
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
      <CustomCardsDialog open={customCardsOpen} onOpenChange={setCustomCardsOpen} />
      <SuppressedCardsDialog 
        open={suppressedCardsOpen} 
        onOpenChange={(open) => {
          setSuppressedCardsOpen(open)
          if (!open) {
            // Refresh the check when dialog closes
            checkSuppressedCards()
          }
        }} 
      />
    </>
  )
}