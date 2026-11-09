import { useState } from 'react'
import { useAuth } from '@/components/auth-provider'
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
import { Settings, LogOut, EyeOff, BarChart2, Search, Table, Shuffle, User } from 'lucide-react'

export function AppHeader() {
  const { isAuthenticated, isAdmin, logout } = useAuth()

  const [settingsOpen, setSettingsOpen] = useState(false)
  const [browseCardsOpen, setBrowseCardsOpen] = useState(false)
  const [conjugationTablesOpen, setConjugationTablesOpen] = useState(false)
  const [suppressedCardsOpen, setSuppressedCardsOpen] = useState(false)
  const [historyOpen, setHistoryOpen] = useState(false)
  const [hanjaDrillsOpen, setHanjaDrillsOpen] = useState(false)
  const [loggingOut, setLoggingOut] = useState(false)
  const [logoutError, setLogoutError] = useState<string | null>(null)

  const handleLogout = async () => {
    setLoggingOut(true)
    setLogoutError(null)
    const result = await logout()
    if (!result.success) setLogoutError(result.error ?? 'Could not log out.')
    setLoggingOut(false)
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
            <DropdownMenuItem onClick={() => setSuppressedCardsOpen(true)}>
              <EyeOff className="mr-2 h-4 w-4" />
              Suppressed Cards
            </DropdownMenuItem>
            <DropdownMenuItem onClick={() => setSettingsOpen(true)}>
              <Settings className="mr-2 h-4 w-4" />
              Settings
            </DropdownMenuItem>
            <DropdownMenuSeparator />
            <DropdownMenuItem onClick={handleLogout} disabled={loggingOut}>
              <LogOut className="mr-2 h-4 w-4" />
              {loggingOut ? 'Logging out…' : 'Log Out'}
            </DropdownMenuItem>
          </DropdownMenuContent>
        </DropdownMenu>
        {logoutError && (
          <p role="alert" className="absolute top-full right-0 mt-2 w-64 rounded-md bg-background p-3 text-sm text-destructive shadow-md">
            {logoutError}
          </p>
        )}
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
        onOpenChange={setSuppressedCardsOpen}
      />
    </>
  )
}
