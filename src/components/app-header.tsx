import { ModeToggle } from '@/components/mode-toggle'
import { SettingsDialog } from '@/components/settings-dialog'

export function AppHeader() {
  return (
    <div className="fixed top-4 right-4 z-50 flex gap-2">
      <SettingsDialog />
      <ModeToggle />
    </div>
  )
}