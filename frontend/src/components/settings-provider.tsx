import { subscribeLiveEvents } from '@/lib/live-events'
import { createRefreshTask } from '@/lib/refresh-task'
import { createContext, useContext, useEffect, useRef, useState } from 'react'
import type { Settings } from '@/lib/types'
import { DEFAULT_SETTINGS, STORAGE_KEYS } from '@/lib/constants'
import { getUserSettings, subscribeReviewQueueChanges, updateUserSettings } from '@/lib/api'
import { useAuth } from '@/components/auth-provider'
import { SettingsWriter, type PersistedSettingsChange, type SettingsSnapshot } from '@/lib/settings-writer'

interface SettingsContextType {
  settings: Settings
  saveStatus: SettingsSnapshot['status']
  saveError: string | null
  updateSettings: (updates: Partial<Settings>) => void
  retrySettings: () => void
  discardSettings: () => void
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
  const [snapshot, setSnapshot] = useState<SettingsSnapshot>(() => {
    const initial = { ...defaultSettings, debugStatusBar: readDebugStatusBar() }
    return { draft: initial, confirmed: initial, status: 'saved', error: null }
  })
  const refreshTask = useRef<ReturnType<typeof createRefreshTask> | null>(null)
  const writerRef = useRef<SettingsWriter | null>(null)
  const { isAuthenticated } = useAuth()

  const createWriter = (initial: Settings) => new SettingsWriter(
    initial,
    async (changes: PersistedSettingsChange) => {
      const result = await updateUserSettings({
        show_hanja_hints: changes.showHanjaHints,
        red_threshold: changes.redThreshold,
        yellow_threshold: changes.yellowThreshold,
        day_boundary_hour: changes.dayBoundaryHour,
        auto_progress_on_correct: changes.autoProgressOnCorrect,
        auto_progress_delay: changes.autoProgressDelay,
        desired_retention: changes.desiredRetention,
        daily_new_card_limit: changes.dailyNewCardLimit,
        history_colorized_area: changes.historyColorizedArea,
        history_colored_dots: changes.historyColoredDots,
        history_threshold_lines: changes.historyThresholdLines,
      })
      if (!result.success) throw new Error('Could not save settings')
    },
    setSnapshot,
    () => refreshTask.current?.request(),
  )
  if (!writerRef.current) writerRef.current = createWriter(snapshot.draft)

  // Fetch settings from backend when authenticated
  useEffect(() => {
    if (!isAuthenticated) {
      writerRef.current?.dispose()
      const initial = { ...defaultSettings, debugStatusBar: readDebugStatusBar() }
      writerRef.current = createWriter(initial)
      setSnapshot(writerRef.current.snapshot())
      return
    }

    const fetchSettings = async (current: () => boolean) => {
      try {
        const writer = writerRef.current!
        const version = writer.getVersion()
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
          debugStatusBar: writer.snapshot().draft.debugStatusBar,
        }
        if (!current()) return
        if (!writer.hydrate(fetchedSettings, version) && !writer.hasPending()) {
          throw new Error('Settings changed during refresh')
        }
      } catch (err) {
        console.error('Failed to fetch settings:', err)
        throw err
      }
    }

    const task = createRefreshTask(fetchSettings)
    refreshTask.current = task
    task.request()
    const local = subscribeReviewQueueChanges(change => { if (change === 'import') task.request() })
    const live = subscribeLiveEvents(event => { if (event.type === 'settings-changed' || event.type === 'state-reset' || event.type === 'resync-required') task.request() })
    return () => { local(); live(); task.dispose(); refreshTask.current = null }
  }, [isAuthenticated])

  const updateSettings = (updates: Partial<Settings>) => {
    const writer = writerRef.current!
    // debugStatusBar is device-local - persist it directly and don't send it
    // below, since UpdateSettingsRequest has no field for it.
    if (updates.debugStatusBar !== undefined) {
      localStorage.setItem(STORAGE_KEYS.DEBUG_STATUS_BAR, String(updates.debugStatusBar))
      writer.setLocal({ debugStatusBar: updates.debugStatusBar })
    }
    if (updates.hasFsrsParameters !== undefined) {
      writer.setLocal({ hasFsrsParameters: updates.hasFsrsParameters })
    }

    // Keep a visible draft while writes serialize. Normalize against the
    // latest draft, including earlier changes React has not rendered yet.
    const { debugStatusBar: _debug, hasFsrsParameters: _fsrs, ...normalizedUpdates } = updates
    const updated = { ...writer.snapshot().draft, ...normalizedUpdates }
    if (updates.redThreshold !== undefined && updated.redThreshold >= updated.yellowThreshold) {
      updated.redThreshold = Math.max(0, updated.yellowThreshold - 5)
      normalizedUpdates.redThreshold = updated.redThreshold
    }
    if (updates.yellowThreshold !== undefined && updated.yellowThreshold <= updated.redThreshold) {
      updated.yellowThreshold = Math.min(100, updated.redThreshold + 5)
      normalizedUpdates.yellowThreshold = updated.yellowThreshold
    }

    if (isAuthenticated) {
      writer.update(normalizedUpdates)
    }
  }

  return (
    <SettingsContext.Provider value={{
      settings: snapshot.draft,
      saveStatus: snapshot.status,
      saveError: snapshot.error,
      updateSettings,
      retrySettings: () => writerRef.current?.retry(),
      discardSettings: () => writerRef.current?.discard(),
    }}>
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
