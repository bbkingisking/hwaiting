import { subscribeLiveEvents } from '@/lib/live-events'
import { createRefreshTask } from '@/lib/refresh-task'
import { createContext, useContext, useEffect, useRef, useState } from 'react'
import type { Settings } from '@/lib/types'
import { DEFAULT_SETTINGS, STORAGE_KEYS } from '@/lib/constants'
import { getUserSettings, subscribeReviewQueueChanges, updateUserSettings } from '@/lib/api'
import { useAuth } from '@/components/auth-provider'

type SettingsChange = Partial<Omit<Settings, 'hasFsrsParameters'>>
type SaveStatus = 'idle' | 'saving' | 'failed'

interface SettingsContextType {
  settings: Settings
  saveStatus: SaveStatus
  saveError: string | null
  saveSettings: (changes: SettingsChange) => Promise<void>
  refreshSettings: () => void
  clearSaveError: () => void
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

function readDebugStatusBar(): boolean {
  if (typeof window === 'undefined') return DEFAULT_SETTINGS.DEBUG_STATUS_BAR
  return localStorage.getItem(STORAGE_KEYS.DEBUG_STATUS_BAR) === 'true'
}

const SettingsContext = createContext<SettingsContextType | undefined>(undefined)

export function SettingsProvider({ children }: { children: React.ReactNode }) {
  const [settings, setSettings] = useState<Settings>(() => ({ ...defaultSettings, debugStatusBar: readDebugStatusBar() }))
  const [saveStatus, setSaveStatus] = useState<SaveStatus>('idle')
  const [saveError, setSaveError] = useState<string | null>(null)
  const saving = useRef(false)
  const refreshTask = useRef<ReturnType<typeof createRefreshTask> | null>(null)
  const refreshAfterSave = useRef(false)
  const { isAuthenticated } = useAuth()

  useEffect(() => {
    if (!isAuthenticated) {
      setSettings({ ...defaultSettings, debugStatusBar: readDebugStatusBar() })
      setSaveStatus('idle')
      setSaveError(null)
      return
    }

    const task = createRefreshTask(async current => {
      const remote = await getUserSettings()
      if (!current()) return
      if (saving.current) {
        refreshAfterSave.current = true
        return
      }
      setSettings({
        showHanjaHints: remote.show_hanja_hints,
        redThreshold: remote.red_threshold,
        yellowThreshold: remote.yellow_threshold,
        dayBoundaryHour: remote.day_boundary_hour,
        autoProgressOnCorrect: remote.auto_progress_on_correct,
        autoProgressDelay: remote.auto_progress_delay ?? DEFAULT_SETTINGS.AUTO_PROGRESS_DELAY,
        desiredRetention: remote.desired_retention,
        dailyNewCardLimit: remote.daily_new_card_limit,
        historyColorizedArea: remote.history_colorized_area,
        historyColoredDots: remote.history_colored_dots,
        historyThresholdLines: remote.history_threshold_lines,
        hasFsrsParameters: remote.has_fsrs_parameters,
        debugStatusBar: readDebugStatusBar(),
      })
    })
    refreshTask.current = task
    task.request()
    const local = subscribeReviewQueueChanges(change => { if (change === 'import') task.request() })
    const live = subscribeLiveEvents(event => {
      if (event.type === 'settings-changed' || event.type === 'state-reset' || event.type === 'resync-required') task.request()
    })
    return () => { local(); live(); task.dispose(); refreshTask.current = null }
  }, [isAuthenticated])

  const saveSettings = async (changes: SettingsChange) => {
    if (saving.current) throw new Error('A settings save is already in progress')
    const { debugStatusBar, ...serverChanges } = changes
    const hasServerChanges = Object.keys(serverChanges).length > 0
    saving.current = true
    setSaveStatus('saving')
    setSaveError(null)
    const controller = new AbortController()
    const timeout = setTimeout(() => controller.abort(), 12000)
    try {
      if (hasServerChanges) {
        const result = await updateUserSettings({
          show_hanja_hints: serverChanges.showHanjaHints,
          red_threshold: serverChanges.redThreshold,
          yellow_threshold: serverChanges.yellowThreshold,
          day_boundary_hour: serverChanges.dayBoundaryHour,
          auto_progress_on_correct: serverChanges.autoProgressOnCorrect,
          auto_progress_delay: serverChanges.autoProgressDelay,
          desired_retention: serverChanges.desiredRetention,
          daily_new_card_limit: serverChanges.dailyNewCardLimit,
          history_colorized_area: serverChanges.historyColorizedArea,
          history_colored_dots: serverChanges.historyColoredDots,
          history_threshold_lines: serverChanges.historyThresholdLines,
        }, controller.signal)
        if (!result.success) throw new Error('Could not save settings')
      }
      if (debugStatusBar !== undefined) localStorage.setItem(STORAGE_KEYS.DEBUG_STATUS_BAR, String(debugStatusBar))
      setSettings(previous => ({ ...previous, ...changes }))
      setSaveStatus('idle')
    } catch (error) {
      const message = controller.signal.aborted
        ? 'Saving settings timed out. Please try again.'
        : error instanceof Error ? error.message : 'Could not save settings'
      setSaveStatus('failed')
      setSaveError(message)
      throw new Error(message)
    } finally {
      clearTimeout(timeout)
      saving.current = false
      if (hasServerChanges || refreshAfterSave.current) refreshTask.current?.request()
      refreshAfterSave.current = false
    }
  }

  return (
    <SettingsContext.Provider value={{
      settings,
      saveStatus,
      saveError,
      saveSettings,
      refreshSettings: () => refreshTask.current?.request(),
      clearSaveError: () => { setSaveStatus('idle'); setSaveError(null) },
    }}>
      {children}
    </SettingsContext.Provider>
  )
}

export function useSettings() {
  const context = useContext(SettingsContext)
  if (!context) throw new Error('useSettings must be used within a SettingsProvider')
  return context
}
