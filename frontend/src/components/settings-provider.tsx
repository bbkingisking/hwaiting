import { createContext, useContext, useEffect, useRef, useState } from 'react'
import type { Settings } from '@/lib/types'
import { DEFAULT_SETTINGS, STORAGE_KEYS } from '@/lib/constants'
import { getUserSettings, updateUserSettings } from '@/lib/api'
import { useAuth } from '@/components/auth-provider'

interface SettingsContextType {
  settings: Settings
  updateSettings: (updates: Partial<Settings>) => void
}

const defaultSettings: Settings = {
  showHanjaHints: DEFAULT_SETTINGS.SHOW_HANJA_HINTS,
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
  debugStatusBar: DEFAULT_SETTINGS.DEBUG_STATUS_BAR,
}

// debugStatusBar is device-local (see its field comment in lib/types.ts) -
// read/written directly against localStorage rather than through the
// backend, same as card-theme-provider.tsx's cardThemeId.
function readDebugStatusBar(): boolean {
  if (typeof window === 'undefined') return DEFAULT_SETTINGS.DEBUG_STATUS_BAR
  return localStorage.getItem(STORAGE_KEYS.DEBUG_STATUS_BAR) === 'true'
}

const SettingsContext = createContext<SettingsContextType | undefined>(undefined)

export function SettingsProvider({ children }: { children: React.ReactNode }) {
  const [settings, setSettings] = useState<Settings>(() => ({ ...defaultSettings, debugStatusBar: readDebugStatusBar() }))
  const settingsRef = useRef(settings)
  const { isAuthenticated } = useAuth()

  // Fetch settings from backend when authenticated
  useEffect(() => {
    if (!isAuthenticated) {
      return
    }

    const fetchSettings = async () => {
      try {
        const userSettings = await getUserSettings()
        const fetchedSettings: Settings = {
          showHanjaHints: userSettings.show_hanja_hints,
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
          // Not part of the backend response - see readDebugStatusBar.
          debugStatusBar: readDebugStatusBar(),
        }
        settingsRef.current = fetchedSettings
        setSettings(fetchedSettings)
      } catch (err) {
        console.error('Failed to fetch settings:', err)
      }
    }

    fetchSettings()
  }, [isAuthenticated])

  const updateSettings = async (updates: Partial<Settings>) => {
    // debugStatusBar is device-local - persist it directly and don't send it
    // below, since UpdateSettingsRequest has no field for it.
    if (updates.debugStatusBar !== undefined) {
      localStorage.setItem(STORAGE_KEYS.DEBUG_STATUS_BAR, String(updates.debugStatusBar))
    }

    // Normalize once so the optimistic state and API payload agree. The ref
    // includes earlier updates even when React batches their renders.
    const normalizedUpdates = { ...updates }
    const updated = { ...settingsRef.current, ...normalizedUpdates }
    if (updates.redThreshold !== undefined && updated.redThreshold >= updated.yellowThreshold) {
      updated.redThreshold = Math.max(0, updated.yellowThreshold - 5)
      normalizedUpdates.redThreshold = updated.redThreshold
    }
    if (updates.yellowThreshold !== undefined && updated.yellowThreshold <= updated.redThreshold) {
      updated.yellowThreshold = Math.min(100, updated.redThreshold + 5)
      normalizedUpdates.yellowThreshold = updated.yellowThreshold
    }

    settingsRef.current = updated
    setSettings(updated)

    // Sync to backend if authenticated
    if (isAuthenticated) {
      try {
        await updateUserSettings({
          show_hanja_hints: normalizedUpdates.showHanjaHints,
          red_threshold: normalizedUpdates.redThreshold,
          yellow_threshold: normalizedUpdates.yellowThreshold,
          day_boundary_hour: normalizedUpdates.dayBoundaryHour,
          auto_progress_on_correct: normalizedUpdates.autoProgressOnCorrect,
          auto_progress_delay: normalizedUpdates.autoProgressDelay,
          desired_retention: normalizedUpdates.desiredRetention,
          daily_new_card_limit: normalizedUpdates.dailyNewCardLimit,
          history_colorized_area: normalizedUpdates.historyColorizedArea,
          history_colored_dots: normalizedUpdates.historyColoredDots,
          history_threshold_lines: normalizedUpdates.historyThresholdLines,
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