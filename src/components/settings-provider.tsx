import { createContext, useContext, useEffect, useState } from 'react'
import type { Settings } from '@/lib/types'
import { DEFAULT_SETTINGS, STORAGE_KEYS } from '@/lib/constants'

interface SettingsContextType {
  settings: Settings
  updateSettings: (updates: Partial<Settings>) => void
}

const defaultSettings: Settings = {
  showPercentage: DEFAULT_SETTINGS.SHOW_PERCENTAGE,
  redThreshold: DEFAULT_SETTINGS.RED_THRESHOLD,
  yellowThreshold: DEFAULT_SETTINGS.YELLOW_THRESHOLD,
}

const SettingsContext = createContext<SettingsContextType | undefined>(undefined)

export function SettingsProvider({ children }: { children: React.ReactNode }) {
  const [settings, setSettings] = useState<Settings>(() => {
    const stored = localStorage.getItem(STORAGE_KEYS.SETTINGS)
    if (stored) {
      try {
        return { ...defaultSettings, ...JSON.parse(stored) }
      } catch {
        return defaultSettings
      }
    }
    return defaultSettings
  })

  useEffect(() => {
    localStorage.setItem(STORAGE_KEYS.SETTINGS, JSON.stringify(settings))
  }, [settings])

  const updateSettings = (updates: Partial<Settings>) => {
    setSettings((prev) => {
      const updated = { ...prev, ...updates }
      
      // Validate that redThreshold < yellowThreshold
      if (updates.redThreshold !== undefined && updated.redThreshold >= updated.yellowThreshold) {
        updated.redThreshold = Math.max(0, updated.yellowThreshold - 5)
      }
      if (updates.yellowThreshold !== undefined && updated.yellowThreshold <= updated.redThreshold) {
        updated.yellowThreshold = Math.min(100, updated.redThreshold + 5)
      }
      
      return updated
    })
  }

  return (
    <SettingsContext.Provider value={{ settings, updateSettings }}>
      {children}
    </SettingsContext.Provider>
  )
}

export function useSettings() {
  const context = useContext(SettingsContext)
  if (!context) {
    throw new Error('useSettings must be used within a SettingsProvider')
  }
  return context
}