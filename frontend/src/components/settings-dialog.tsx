import { Trash2, Download, Upload } from 'lucide-react'
import { useState, useEffect } from 'react'
import { Button } from '@/components/ui/button'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { Label } from '@/components/ui/label'
import { Slider } from '@/components/ui/slider'
import { Switch } from '@/components/ui/switch'
import {
  Accordion,
  AccordionItem,
  AccordionTrigger,
  AccordionContent,
} from '@/components/ui/accordion'
import { useSettings } from '@/components/settings-provider'
import { useAuth } from '@/components/auth-provider'
import { useCardTheme } from '@/components/card-theme-provider'
import { CARD_THEMES } from '@/lib/card-themes'
import { THRESHOLD_CONSTRAINTS, AUTO_PROGRESS_DELAY_CONSTRAINTS, DESIRED_RETENTION_CONSTRAINTS } from '@/lib/constants'
import { addPasskey, deletePasskey, exportUserData, importUserData, listPasskeys, optimizeFsrs, resetFsrsParameters, type ImportResponse, type PasskeySummary } from '@/lib/api'
import { cn } from '@/lib/utils'

interface SettingsDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
}

export function SettingsDialog({ open, onOpenChange }: SettingsDialogProps) {
  const { settings, updateSettings } = useSettings()
  const { token, passkeysEnabled } = useAuth()
  const { cardThemeId, setCardThemeId } = useCardTheme()
  const [isExporting, setIsExporting] = useState(false)
  const [isImporting, setIsImporting] = useState(false)
  const [importMessage, setImportMessage] = useState<{ type: 'success' | 'error', text: string } | null>(null)
  const [isOptimizing, setIsOptimizing] = useState(false)
  const [optimizeMessage, setOptimizeMessage] = useState<{ type: 'success' | 'error', text: string } | null>(null)
  const [showImportAlert, setShowImportAlert] = useState(false)
  const [pendingImportFile, setPendingImportFile] = useState<File | null>(null)
  const [passkeys, setPasskeys] = useState<PasskeySummary[]>([])
  const [isLoadingPasskeys, setIsLoadingPasskeys] = useState(false)
  const [isAddingPasskey, setIsAddingPasskey] = useState(false)
  const [passkeyMessage, setPasskeyMessage] = useState<{ type: 'success' | 'error', text: string } | null>(null)

  // Track user's preferred limit when they toggle suppress on/off
  const [preferredLimit, setPreferredLimit] = useState(20)

  const fetchPasskeys = async () => {
    if (!token) return

    setIsLoadingPasskeys(true)
    try {
      const response = await listPasskeys()
      setPasskeys(response.passkeys)
    } catch (error) {
      console.error('Failed to fetch passkeys:', error)
    } finally {
      setIsLoadingPasskeys(false)
    }
  }

  const handleAddPasskey = async () => {
    setIsAddingPasskey(true)
    setPasskeyMessage(null)
    try {
      await addPasskey()
      setPasskeyMessage({ type: 'success', text: 'Passkey added' })
      await fetchPasskeys()
    } catch (error) {
      setPasskeyMessage({
        type: 'error',
        text: error instanceof Error ? error.message : 'Failed to add passkey',
      })
    } finally {
      setIsAddingPasskey(false)
    }
  }

  const handleDeletePasskey = async (id: number) => {
    setPasskeyMessage(null)
    try {
      await deletePasskey(id)
      await fetchPasskeys()
    } catch (error) {
      setPasskeyMessage({
        type: 'error',
        text: error instanceof Error ? error.message : 'Failed to remove passkey',
      })
    }
  }

  const handleExport = async () => {
    setIsExporting(true)
    try {
      await exportUserData()
    } catch (error) {
      console.error('Failed to export data:', error)
    } finally {
      setIsExporting(false)
    }
  }

  const handleImportFileSelect = (event: React.ChangeEvent<HTMLInputElement>) => {
    const file = event.target.files?.[0]
    if (!file) return

    // Store the file and show confirmation alert
    setPendingImportFile(file)
    setShowImportAlert(true)
    // Reset the file input
    event.target.value = ''
  }

  const handleImportConfirm = async () => {
    if (!pendingImportFile) return

    setShowImportAlert(false)
    setIsImporting(true)
    setImportMessage(null)
    try {
      const result: ImportResponse = await importUserData(pendingImportFile, true)
      setImportMessage({
        type: 'success',
        text: `Successfully imported ${result.stats.card_states_derived} card states, ${result.stats.reviews_imported} reviews, and ${result.stats.suppressed_cards_imported} suppressed cards`
      })
    } catch (error) {
      console.error('Failed to import data:', error)
      setImportMessage({
        type: 'error',
        text: error instanceof Error ? error.message : 'Failed to import data'
      })
    } finally {
      setIsImporting(false)
      setPendingImportFile(null)
    }
  }

  const handleImportCancel = () => {
    setShowImportAlert(false)
    setPendingImportFile(null)
  }

  const handleOptimize = async () => {
    setIsOptimizing(true)
    setOptimizeMessage(null)
    try {
      const result = await optimizeFsrs()
      setOptimizeMessage({
        type: 'success',
        text: `Optimized from ${result.review_count} reviews`
      })
      updateSettings({ hasFsrsParameters: true })
    } catch (error) {
      console.error('Failed to optimize FSRS parameters:', error)
      setOptimizeMessage({
        type: 'error',
        text: error instanceof Error ? error.message : 'Failed to optimize parameters'
      })
    } finally {
      setIsOptimizing(false)
    }
  }

  const handleResetFsrs = async () => {
    try {
      await resetFsrsParameters()
      setOptimizeMessage(null)
      updateSettings({ hasFsrsParameters: false })
    } catch (error) {
      console.error('Failed to reset FSRS parameters:', error)
    }
  }

  // Initialize preferred limit from current settings
  useEffect(() => {
    if (settings.dailyNewCardLimit > 0) {
      setPreferredLimit(settings.dailyNewCardLimit)
    }
  }, [])

  useEffect(() => {
    if (open && passkeysEnabled) {
      fetchPasskeys()
    }
  }, [open, passkeysEnabled])

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-106.25 flex flex-col max-h-[85vh]">
        <DialogHeader>
          <DialogTitle>Settings</DialogTitle>
          <DialogDescription>
            Customize your flashcard experience
          </DialogDescription>
        </DialogHeader>
        <Accordion className="overflow-y-auto pr-1">
          {/* General */}
          <AccordionItem value="general">
            <AccordionTrigger>General</AccordionTrigger>
            <AccordionContent className="flex flex-col gap-4">
              <div className="flex flex-col gap-2">
                <div className="flex items-center justify-between">
                  <Label htmlFor="auto-progress" className="flex-1">
                    Auto-progress on correct
                  </Label>
                  <Switch
                    id="auto-progress"
                    checked={settings.autoProgressOnCorrect}
                    onCheckedChange={(checked) => updateSettings({ autoProgressOnCorrect: checked })}
                  />
                </div>
                <p className="text-xs text-muted-foreground">
                  Skip feedback screen and move to next card when answered correctly
                </p>
              </div>

              {settings.autoProgressOnCorrect && (
                <div className="flex flex-col gap-2">
                  <div className="flex items-center justify-between">
                    <Label id="auto-progress-delay-label">Auto-progress delay</Label>
                    <span className="text-sm text-muted-foreground">
                      {settings.autoProgressDelay === 0 ? 'Instant' : `${(settings.autoProgressDelay / 1000).toFixed(1)}s`}
                    </span>
                  </div>
                  <Slider
                    aria-labelledby="auto-progress-delay-label"
                    min={AUTO_PROGRESS_DELAY_CONSTRAINTS.MIN}
                    max={AUTO_PROGRESS_DELAY_CONSTRAINTS.MAX}
                    step={AUTO_PROGRESS_DELAY_CONSTRAINTS.STEP}
                    value={settings.autoProgressDelay}
                    onValueChange={(value) => updateSettings({ autoProgressDelay: value as number })}
                  />
                  <p className="text-xs text-muted-foreground">
                    How long to show correct answer before moving to next card
                  </p>
                </div>
              )}
            </AccordionContent>
          </AccordionItem>

          {/* Appearance */}
          <AccordionItem value="appearance">
            <AccordionTrigger>Appearance</AccordionTrigger>
            <AccordionContent className="flex flex-col gap-6">
              <section aria-labelledby="settings-cards-heading" className="flex flex-col gap-4">
                <h4 id="settings-cards-heading" className="font-semibold">Cards</h4>
                <div className="flex flex-col gap-2">
                  <div className="flex items-center justify-between">
                    <Label htmlFor="show-hanja-hints" className="flex-1">
                      Hanja hints
                    </Label>
                    <Switch
                      id="show-hanja-hints"
                      checked={settings.showHanjaHints}
                      onCheckedChange={(checked) => updateSettings({ showHanjaHints: checked })}
                    />
                  </div>
                  <p className="text-xs text-muted-foreground">
                    Show Hanja hints above the answer, including related words and readings
                  </p>
                </div>
              </section>

              <section aria-labelledby="settings-status-bar-heading" className="flex flex-col gap-4">
                <h4 id="settings-status-bar-heading" className="font-semibold">Status bar</h4>
                <div role="group" aria-labelledby="settings-accuracy-colours-heading" className="flex flex-col gap-4">
                  <h5 id="settings-accuracy-colours-heading" className="font-medium">Accuracy colours</h5>
                  <div className="flex flex-col gap-2">
                    <div className="flex items-center justify-between">
                      <Label id="red-threshold-label">Red threshold (below)</Label>
                      <span className="text-sm text-muted-foreground">{settings.redThreshold}%</span>
                    </div>
                    <Slider
                      aria-labelledby="red-threshold-label"
                      min={THRESHOLD_CONSTRAINTS.MIN}
                      max={THRESHOLD_CONSTRAINTS.MAX}
                      step={THRESHOLD_CONSTRAINTS.STEP}
                      value={settings.redThreshold}
                      onValueChange={(value) => updateSettings({ redThreshold: value as number })}
                      className="**:[[data-slot=slider-thumb]]:bg-destructive"
                    />
                    <p className="text-xs text-muted-foreground">
                      Overall accuracy in the status bar is red below this percentage
                    </p>
                  </div>

                  <div className="flex flex-col gap-2">
                    <div className="flex items-center justify-between">
                      <Label id="yellow-threshold-label">Yellow threshold (below)</Label>
                      <span className="text-sm text-muted-foreground">{settings.yellowThreshold}%</span>
                    </div>
                    <Slider
                      aria-labelledby="yellow-threshold-label"
                      min={THRESHOLD_CONSTRAINTS.MIN}
                      max={THRESHOLD_CONSTRAINTS.MAX}
                      step={THRESHOLD_CONSTRAINTS.STEP}
                      value={settings.yellowThreshold}
                      onValueChange={(value) => updateSettings({ yellowThreshold: value as number })}
                      className="**:[[data-slot=slider-thumb]]:bg-yellow-600"
                    />
                    <p className="text-xs text-muted-foreground">
                      Overall accuracy is yellow between the two thresholds, and green at or above this value
                    </p>
                  </div>
                </div>

                <div className="flex flex-col gap-2">
                  <div className="flex items-center justify-between">
                    <Label htmlFor="debug-status-bar" className="flex-1">
                      Debug info
                    </Label>
                    <Switch
                      id="debug-status-bar"
                      checked={settings.debugStatusBar}
                      onCheckedChange={(checked) => updateSettings({ debugStatusBar: checked })}
                    />
                  </div>
                  <p className="text-xs text-muted-foreground">
                    Show the build version, answer-check time, and card ID. Saved to this browser only.
                  </p>
                </div>
              </section>

              <section aria-labelledby="settings-themes-heading" className="flex flex-col gap-4">
                <h4 id="settings-themes-heading" className="font-semibold">Themes</h4>
                <div role="group" aria-label="Card theme" className="grid grid-cols-2 gap-2">
                  {CARD_THEMES.map((cardTheme) => (
                    <button
                      key={cardTheme.id}
                      type="button"
                      aria-pressed={cardThemeId === cardTheme.id}
                      onClick={() => setCardThemeId(cardTheme.id)}
                      className={cn(
                        'flex items-center gap-2 rounded-md border p-2 text-left text-sm transition-colors hover:bg-accent',
                        cardThemeId === cardTheme.id
                          ? 'border-primary ring-1 ring-primary'
                          : 'border-border',
                      )}
                    >
                      <span
                        className="h-6 w-6 shrink-0 rounded-full border border-black/10"
                        style={{
                          background: cardTheme.swatch.bg,
                          boxShadow: `inset 0 0 0 2px ${cardTheme.swatch.accent}`,
                        }}
                      />
                      <span className="truncate">{cardTheme.name}</span>
                    </button>
                  ))}
                </div>
                <p className="text-xs text-muted-foreground">
                  Changes how the review card looks. Saved to this browser only.
                </p>
              </section>
            </AccordionContent>
          </AccordionItem>

          {/* Review Settings */}
          <AccordionItem value="review-settings">
            <AccordionTrigger>Review settings</AccordionTrigger>
            <AccordionContent className="flex flex-col gap-4">
              <div className="flex flex-col gap-2">
                <div className="flex items-center justify-between">
                  <Label htmlFor="suppress-new-cards" className="flex-1">
                    Suppress new cards
                  </Label>
                  <Switch
                    id="suppress-new-cards"
                    checked={settings.dailyNewCardLimit === 0}
                    onCheckedChange={(checked) => {
                      if (checked) {
                        // Save current limit before suppressing
                        setPreferredLimit(settings.dailyNewCardLimit || 20)
                        updateSettings({ dailyNewCardLimit: 0 })
                      } else {
                        // Restore preferred limit
                        updateSettings({ dailyNewCardLimit: preferredLimit })
                      }
                    }}
                  />
                </div>
                <p className="text-xs text-muted-foreground">
                  Only show cards you've already reviewed
                </p>
              </div>

              {settings.dailyNewCardLimit > 0 && (
                <div className="flex flex-col gap-2">
                  <div className="flex items-center justify-between">
                    <Label id="daily-new-card-limit-label">Daily new card limit</Label>
                    <span className="text-sm text-muted-foreground">
                      {settings.dailyNewCardLimit}
                    </span>
                  </div>
                  <Slider
                    aria-labelledby="daily-new-card-limit-label"
                    min={1}
                    max={100}
                    step={1}
                    value={settings.dailyNewCardLimit}
                    onValueChange={(value) => {
                      const limit = value as number
                      updateSettings({ dailyNewCardLimit: limit })
                      setPreferredLimit(limit)
                    }}
                  />
                  <p className="text-xs text-muted-foreground">
                    Maximum number of new cards to learn per day. Does not affect cards due for review.
                  </p>
                </div>
              )}

              <div className="flex flex-col gap-2">
                <div className="flex items-center justify-between">
                  <Label id="day-boundary-label">Day ends at</Label>
                  <span className="text-sm text-muted-foreground">
                    {settings.dayBoundaryHour.toString().padStart(2, '0')}:00
                  </span>
                </div>
                <Slider
                  aria-labelledby="day-boundary-label"
                  min={0}
                  max={23}
                  step={1}
                  value={settings.dayBoundaryHour}
                  onValueChange={(value) => updateSettings({ dayBoundaryHour: value as number })}
                />
                <p className="text-xs text-muted-foreground">
                  Reviews before this hour count as the previous day
                </p>
              </div>

              <div className="flex flex-col gap-2">
                <div className="flex items-center justify-between">
                  <Label id="desired-retention-label">Desired retention</Label>
                  <span className="text-sm text-muted-foreground">
                    {Math.round(settings.desiredRetention * 100)}%
                  </span>
                </div>
                <Slider
                  aria-labelledby="desired-retention-label"
                  min={DESIRED_RETENTION_CONSTRAINTS.MIN * 100}
                  max={DESIRED_RETENTION_CONSTRAINTS.MAX * 100}
                  step={DESIRED_RETENTION_CONSTRAINTS.STEP * 100}
                  value={settings.desiredRetention * 100}
                  onValueChange={(value) => updateSettings({ desiredRetention: (value as number) / 100 })}
                />
                <p className="text-xs text-muted-foreground">
                  Target probability of recalling a card at review time. Higher = more frequent reviews, lower = fewer reviews
                </p>
              </div>

              <div className="flex flex-col gap-2">
                <div className="flex items-center gap-2">
                  <Button
                    onClick={handleOptimize}
                    disabled={isOptimizing}
                    variant="outline"
                  >
                    {isOptimizing ? 'Optimizing...' : 'Optimize scheduling'}
                  </Button>
                  {settings.hasFsrsParameters && (
                    <Button
                      onClick={handleResetFsrs}
                      variant="ghost"
                      size="sm"
                    >
                      Reset to defaults
                    </Button>
                  )}
                </div>
                {settings.hasFsrsParameters && !optimizeMessage && (
                  <p className="text-xs text-green-600 dark:text-green-500">
                    Using personalized parameters
                  </p>
                )}
                {optimizeMessage && (
                  <p className={`text-xs ${optimizeMessage.type === 'success' ? 'text-green-600 dark:text-green-500' : 'text-destructive'}`}>
                    {optimizeMessage.text}
                  </p>
                )}
                <p className="text-xs text-muted-foreground">
                  Train the scheduling algorithm on your review history for better intervals
                </p>
              </div>
            </AccordionContent>
          </AccordionItem>

          {/* Import/export data */}
          <AccordionItem value="data">
            <AccordionTrigger>Import/export data</AccordionTrigger>
            <AccordionContent className="flex flex-col gap-4">
              <div className="flex gap-2">
                <Button
                  onClick={handleExport}
                  disabled={isExporting}
                  variant="outline"
                >
                  <Download data-icon="inline-start" />
                  {isExporting ? 'Exporting...' : 'Export Data'}
                </Button>
                <Button
                  variant="outline"
                  disabled={isImporting}
                  onClick={() => document.getElementById('import-file')?.click()}
                >
                  <Upload data-icon="inline-start" />
                  {isImporting ? 'Importing...' : 'Import Data'}
                </Button>
                <input
                  id="import-file"
                  type="file"
                  accept="application/json,.json"
                  onChange={handleImportFileSelect}
                  className="hidden"
                />
              </div>
              {importMessage && (
                <div className={`text-sm ${importMessage.type === 'success' ? 'text-green-600 dark:text-green-500' : 'text-destructive'}`}>
                  {importMessage.text}
                </div>
              )}
            </AccordionContent>
          </AccordionItem>

          {/* Passkeys - omitted entirely when the server doesn't offer them
              (HWAITING_RP_ID/HWAITING_RP_ORIGINS unset), same as the auth
              dialog's method switch. */}
          {passkeysEnabled && (
          <AccordionItem value="passkeys">
            <AccordionTrigger>Passkeys</AccordionTrigger>
            <AccordionContent className="flex flex-col gap-4">
              <p className="text-xs text-muted-foreground">
                Add a passkey from another device to sign in there too. You need at least one at all times.
              </p>
              <Button
                onClick={handleAddPasskey}
                disabled={isAddingPasskey}
                variant="outline"
                className="self-start"
              >
                {isAddingPasskey ? 'Waiting for passkey…' : 'Add a passkey'}
              </Button>
              {passkeyMessage && (
                <p className={`text-xs ${passkeyMessage.type === 'success' ? 'text-green-600 dark:text-green-500' : 'text-destructive'}`}>
                  {passkeyMessage.text}
                </p>
              )}
              <div className="flex flex-col gap-2 max-h-64 overflow-y-auto">
                {isLoadingPasskeys ? (
                  <p className="text-sm text-muted-foreground">Loading...</p>
                ) : passkeys.length === 0 ? (
                  <p className="text-sm text-muted-foreground">No passkeys yet</p>
                ) : (
                  passkeys.map((passkey) => (
                    <div
                      key={passkey.id}
                      className="flex items-center justify-between p-2 border rounded-md"
                    >
                      <div className="flex-1 text-sm">
                        <p>Added {new Date(passkey.created_at).toLocaleDateString()}</p>
                        <p className="text-xs text-muted-foreground">
                          {passkey.last_used_at
                            ? `Last used ${new Date(passkey.last_used_at).toLocaleDateString()}`
                            : 'Never used'}
                        </p>
                      </div>
                      <Button
                        size="icon"
                        variant="ghost"
                        aria-label="Remove this passkey"
                        onClick={() => handleDeletePasskey(passkey.id)}
                      >
                        <Trash2 className="h-4 w-4" />
                      </Button>
                    </div>
                  ))
                )}
              </div>
            </AccordionContent>
          </AccordionItem>
          )}
        </Accordion>
      </DialogContent>

      {/* Import Confirmation Dialog */}
      <Dialog open={showImportAlert} onOpenChange={setShowImportAlert}>
        <DialogContent className="sm:max-w-106.25">
          <DialogHeader>
            <DialogTitle>⚠️ Warning: Import Data</DialogTitle>
            <DialogDescription>
              This will replace all of your current learning data with the data from the imported file.
            </DialogDescription>
          </DialogHeader>
          <div className="space-y-4 py-4">
            <p className="text-sm text-muted-foreground">
              The following will be replaced:
            </p>
            <ul className="text-sm text-muted-foreground list-disc list-inside space-y-1">
              <li>Card states (FSRS stability, difficulty, due dates)</li>
              <li>Review history (all past review records)</li>
              <li>Suppressed cards</li>
              <li>Settings (preferences)</li>
            </ul>
            <p className="text-sm font-semibold">
              This action cannot be undone. Make sure you have a backup of your current data before proceeding.
            </p>
          </div>
          <div className="flex justify-end gap-2">
            <Button
              variant="outline"
              onClick={handleImportCancel}
            >
              Cancel
            </Button>
            <Button
              variant="destructive"
              onClick={handleImportConfirm}
            >
              Import and Replace Data
            </Button>
          </div>
        </DialogContent>
      </Dialog>
    </Dialog>
  )
}
