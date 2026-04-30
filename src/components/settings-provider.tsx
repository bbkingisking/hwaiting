import { createContext, useContext, useEffect, useState } from 'react'
import type { Settings } from '@/lib/types'
import { DEFAULT_SETTINGS } from '@/lib/constants'
import { getUserSettings, updateUserSettings } from '@/lib/api'
import { useAuth } from '@/components/auth-provider'

interface SettingsContextType {
  settings: Settings
  updateSettings: (updates: Partial<Settings>) => void
}

const defaultSettings: Settings = {
  showPercentage: DEFAULT_SETTINGS.SHOW_PERCENTAGE,
  redThreshold: DEFAULT_SETTINGS.RED_THRESHOLD,
  yellowThreshold: DEFAULT_SETTINGS.YELLOW_THRESHOLD,
  dayBoundaryHour: DEFAULT_SETTINGS.DAY_BOUNDARY_HOUR,
  autoProgressOnCorrect: DEFAULT_SETTINGS.AUTO_PROGRESS_ON_CORRECT,
}

const SettingsContext = createContext<SettingsContextType | undefined>(undefined)

export function SettingsProvider({ children }: { children: React.ReactNode }) {
  const [settings, setSettings] = useState<Settings>(defaultSettings)
  const [loading, setLoading] = useState(true)
  const { isAuthenticated } = useAuth()

  // Fetch settings from backend when authenticated
  useEffect(() => {
    if (!isAuthenticated) {
      setLoading(false)
      return
    }

    const fetchSettings = async () => {
      try {
        const userSettings = await getUserSettings()
        setSettings({
          showPercentage: userSettings.show_percentage,
          redThreshold: userSettings.red_threshold,
          yellowThreshold: userSettings.yellow_threshold,
          dayBoundaryHour: userSettings.day_boundary_hour,
          autoProgressOnCorrect: userSettings.auto_progress_on_correct,
        })
      } catch (err) {
        console.error('Failed to fetch settings:', err)
      } finally {
        setLoading(false)
      }
    }

    fetchSettings()
  }, [isAuthenticated])

  const updateSettings = async (updates: Partial<Settings>) => {
    // Optimistic update
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

    // Sync to backend if authenticated
    if (isAuthenticated) {
      try {
        await updateUserSettings({
          show_percentage: updates.showPercentage,
          red_threshold: updates.redThreshold,
          yellow_threshold: updates.yellowThreshold,
          day_boundary_hour: updates.dayBoundaryHour,
          auto_progress_on_correct: updates.autoProgressOnCorrect,
        })
      } catch (err) {
        console.error('Failed to update settings:', err)
        // Could revert optimistic update here if needed
      }
    }
  }

  return (
    <SettingsContext.Provider value={{ settings, updateSettings }}>
      {loading ? null : children}
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