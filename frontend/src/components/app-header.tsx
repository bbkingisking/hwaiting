import { useState, useEffect } from 'react'
import { useAuth } from '@/components/auth-provider'
import { listSuppressedCards } from '@/lib/api'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'
import { SettingsDialog } from '@/components/settings-dialog'
import { SuppressedCardsDialog } from '@/components/suppressed-cards-dialog'
import { ReviewHistoryDialog } from '@/components/review-history-dialog'
import { BrowseCardsDialog } from '@/components/browse-cards-dialog'
import { ConjugationTablesDialog } from '@/components/conjugation-tables-dialog'
import { HanjaDrillsDialog } from '@/components/hanja-drills-dialog'
import { Settings, Moon, Sun, LogOut, EyeOff, BarChart2, Search, Table, Shuffle, User } from 'lucide-react'
import { useTheme } from '@/components/theme-provider'



export function AppHeader() {
  const { isAuthenticated, isAdmin, logout } = useAuth()
  const { theme, setTheme } = useTheme()

  const [settingsOpen, setSettingsOpen] = useState(false)
  const [browseCardsOpen, setBrowseCardsOpen] = useState(false)
  const [conjugationTablesOpen, setConjugationTablesOpen] = useState(false)
  const [suppressedCardsOpen, setSuppressedCardsOpen] = useState(false)
  const [historyOpen, setHistoryOpen] = useState(false)
  const [hanjaDrillsOpen, setHanjaDrillsOpen] = useState(false)
  const [hasSuppressedCards, setHasSuppressedCards] = useState(false)

  const checkSuppressedCards = () => {
    listSuppressedCards()
      .then(response => setHasSuppressedCards(response.cards.length > 0))
      .catch(err => console.error('Error checking suppressed cards:', err))
  }

  useEffect(() => {
    if (isAuthenticated) {
      checkSuppressedCards()
    } else {
      setHasSuppressedCards(false)
    }
  }, [isAuthenticated])

  const toggleTheme = () => {
    setTheme(theme === 'dark' ? 'light' : 'dark')
  }

  if (!isAuthenticated) {
    return null
  }

  return (
    <>
      <header className="fixed top-4 right-4 z-50 flex items-center gap-3">
        <DropdownMenu>
          <DropdownMenuTrigger aria-label="Account menu" className="p-2 rounded-md bg-background/80 backdrop-blur-sm border border-border hover:bg-accent hover:text-accent-foreground transition-colors outline-none focus-visible:ring-2 focus-visible:ring-ring">
            <User className="h-4 w-4" />
          </DropdownMenuTrigger>
          <DropdownMenuContent align="end" className="w-[calc(100vw-2rem)] sm:w-48">
            <DropdownMenuItem onClick={() => setHistoryOpen(true)}>
              <BarChart2 className="mr-2 h-4 w-4" />
              Review History
            </DropdownMenuItem>
            <DropdownMenuItem onClick={() => setHanjaDrillsOpen(true)}>
              <Shuffle className="mr-2 h-4 w-4" />
              Hanja Drills
            </DropdownMenuItem>
            {isAdmin && (
              <DropdownMenuItem onClick={() => setBrowseCardsOpen(true)}>
                <Search className="mr-2 h-4 w-4" />
                Browse Cards
              </DropdownMenuItem>
            )}
            {isAdmin && (
              <DropdownMenuItem onClick={() => setConjugationTablesOpen(true)}>
                <Table className="mr-2 h-4 w-4" />
                Conjugation Tables
              </DropdownMenuItem>
            )}
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
      </header>

      <SettingsDialog open={settingsOpen} onOpenChange={setSettingsOpen} />
      {isAdmin && (
        <BrowseCardsDialog open={browseCardsOpen} onOpenChange={setBrowseCardsOpen} />
      )}
      {isAdmin && (
        <ConjugationTablesDialog open={conjugationTablesOpen} onOpenChange={setConjugationTablesOpen} />
      )}
      <ReviewHistoryDialog open={historyOpen} onOpenChange={setHistoryOpen} />
      <HanjaDrillsDialog open={hanjaDrillsOpen} onOpenChange={setHanjaDrillsOpen} />
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