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
  autoProgressDelay: DEFAULT_SETTINGS.AUTO_PROGRESS_DELAY,
  desiredRetention: DEFAULT_SETTINGS.DESIRED_RETENTION,
  dailyNewCardLimit: DEFAULT_SETTINGS.DAILY_NEW_CARD_LIMIT,
  historyColorizedArea: DEFAULT_SETTINGS.HISTORY_COLORIZED_AREA,
  historyColoredDots: DEFAULT_SETTINGS.HISTORY_COLORED_DOTS,
  historyThresholdLines: DEFAULT_SETTINGS.HISTORY_THRESHOLD_LINES,
  hasFsrsParameters: false,
}

const SettingsContext = createContext<SettingsContextType | undefined>(undefined)

export function SettingsProvider({ children }: { children: React.ReactNode }) {
  const [settings, setSettings] = useState<Settings>(defaultSettings)
  const { isAuthenticated } = useAuth()

  // Fetch settings from backend when authenticated
  useEffect(() => {
    if (!isAuthenticated) {
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
          autoProgressDelay: userSettings.auto_progress_delay ?? DEFAULT_SETTINGS.AUTO_PROGRESS_DELAY,
          desiredRetention: userSettings.desired_retention,
          dailyNewCardLimit: userSettings.daily_new_card_limit,
          historyColorizedArea: userSettings.history_colorized_area,
          historyColoredDots: userSettings.history_colored_dots,
          historyThresholdLines: userSettings.history_threshold_lines,
          hasFsrsParameters: userSettings.has_fsrs_parameters,
        })
      } catch (err) {
        console.error('Failed to fetch settings:', err)
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
          auto_progress_delay: updates.autoProgressDelay,
          desired_retention: updates.desiredRetention,
          daily_new_card_limit: updates.dailyNewCardLimit,
          history_colorized_area: updates.historyColorizedArea,
          history_colored_dots: updates.historyColoredDots,
          history_threshold_lines: updates.historyThresholdLines,
        })
      } catch (err) {
        console.error('Failed to update settings:', err)
        // Could revert optimistic update here if needed
      }
    }
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