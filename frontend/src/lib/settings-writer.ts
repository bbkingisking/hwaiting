import type { Settings } from './types'

export type PersistedSettingsChange = Partial<Omit<Settings, 'debugStatusBar' | 'hasFsrsParameters'>>
export type SettingsSaveStatus = 'saved' | 'saving' | 'failed'
export type SettingsSnapshot = {
  draft: Settings
  confirmed: Settings
  status: SettingsSaveStatus
  error: string | null
}

// A single writer owns the pending patch. Edits made during an in-flight
// request replace older values in the next patch, so the last local action
// for each field is the last one sent to the server.
export class SettingsWriter {
  private draft: Settings
  private confirmed: Settings
  private pending: PersistedSettingsChange = {}
  private writing = false
  private failure: string | null = null
  private disposed = false
  private version = 0

  constructor(
    initial: Settings,
    private save: (changes: PersistedSettingsChange) => Promise<void>,
    private changed: (snapshot: SettingsSnapshot) => void,
    private settled: () => void,
  ) {
    this.draft = initial
    this.confirmed = initial
  }

  snapshot(): SettingsSnapshot {
    return {
      draft: this.draft,
      confirmed: this.confirmed,
      status: this.failure ? 'failed' : this.writing || Object.keys(this.pending).length ? 'saving' : 'saved',
      error: this.failure,
    }
  }

  getVersion() { return this.version }

  hasPending() { return this.writing || Object.keys(this.pending).length > 0 }

  update(changes: PersistedSettingsChange) {
    if (this.disposed || Object.keys(changes).length === 0) return
    this.version++
    this.draft = { ...this.draft, ...changes }
    this.pending = { ...this.pending, ...changes }
    this.failure = null
    this.emit()
    void this.drain()
  }

  // These fields are confirmed by localStorage or by a different successful
  // server operation (FSRS optimization/reset), not by PATCH /settings.
  setLocal(changes: Pick<Partial<Settings>, 'debugStatusBar' | 'hasFsrsParameters'>) {
    if (this.disposed) return
    this.version++
    this.draft = { ...this.draft, ...changes }
    this.confirmed = { ...this.confirmed, ...changes }
    this.emit()
  }

  // A GET that began before a local edit must never overwrite its draft.
  hydrate(settings: Settings, readStartedAtVersion: number): boolean {
    if (this.disposed || this.hasPending() || this.version !== readStartedAtVersion) return false
    this.version++
    this.draft = settings
    this.confirmed = settings
    this.failure = null
    this.emit()
    return true
  }

  retry() {
    if (this.disposed || !this.failure) return
    this.failure = null
    this.emit()
    void this.drain()
  }

  discard() {
    if (this.disposed || !this.failure || this.writing) return
    this.version++
    this.pending = {}
    this.draft = this.confirmed
    this.failure = null
    this.emit()
    this.settled() // A failed response may still have committed. Read back.
  }

  dispose() { this.disposed = true }

  private emit() {
    if (!this.disposed) this.changed(this.snapshot())
  }

  private async drain() {
    if (this.writing || this.disposed) return
    this.writing = true
    this.emit()
    while (!this.disposed && Object.keys(this.pending).length > 0) {
      const patch = this.pending
      this.pending = {}
      try {
        await this.save(patch)
        if (this.disposed) return
        this.version++ // Reject any GET that started before this acknowledgement.
        this.confirmed = { ...this.confirmed, ...patch }
        this.emit()
      } catch (error) {
        if (this.disposed) return
        // If the request committed but its response was lost, repeating this
        // idempotent PATCH is safe. Newer edits win over the failed values.
        this.pending = { ...patch, ...this.pending }
        this.failure = error instanceof Error ? error.message : 'Could not save settings'
        break
      }
    }
    this.writing = false
    this.emit()
    if (!this.failure && !this.disposed) this.settled()
  }
}
