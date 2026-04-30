import { ModeToggle } from '@/components/mode-toggle'
import { SettingsDialog } from '@/components/settings-dialog'
import { useAuth } from '@/components/auth-provider'

export function AppHeader() {
  const { username } = useAuth()

  return (
    <div className="fixed top-4 right-4 z-50 flex items-center gap-2">
      {username && (
        <div className="px-3 py-1.5 rounded-md bg-background/80 backdrop-blur-sm border border-border">
          <span className="text-sm font-medium text-foreground">{username}</span>
        </div>
      )}
      {username && (
        <div className="px-2 py-1.5 rounded-md bg-background/80 backdrop-blur-sm border border-border">
          <span className="fi fi-kr text-lg"></span>
        </div>
      )}
      <SettingsDialog />
      <ModeToggle />
    </div>
  )
}