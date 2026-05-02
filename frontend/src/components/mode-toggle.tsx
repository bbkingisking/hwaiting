import { Moon, Sun, Monitor } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { useTheme } from '@/components/theme-provider'

const order = ['light', 'dark', 'system'] as const

const icons = {
  light: Sun,
  dark: Moon,
  system: Monitor,
}

const labels = {
  light: 'Light mode',
  dark: 'Dark mode',
  system: 'System theme',
}

export function ModeToggle() {
  const { theme, setTheme } = useTheme()

  function cycle() {
    const idx = order.indexOf(theme)
    setTheme(order[(idx + 1) % order.length])
  }

  const Icon = icons[theme]

  return (
    <Button variant="ghost" size="icon" onClick={cycle} aria-label={labels[theme]}>
      <Icon className="size-4" />
    </Button>
  )
}
