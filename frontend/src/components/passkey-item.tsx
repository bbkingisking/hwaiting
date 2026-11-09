import { Pencil, Trash2 } from 'lucide-react'
import { useState } from 'react'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { updatePasskeyLabel, type PasskeySummary } from '@/lib/api'

interface PasskeyItemProps {
  passkey: PasskeySummary
  onUpdate: (passkey: PasskeySummary) => void
  onDelete: (id: number) => void
}

export function PasskeyItem({ passkey, onUpdate, onDelete }: PasskeyItemProps) {
  const [editing, setEditing] = useState(false)
  const [label, setLabel] = useState('')
  const [saving, setSaving] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const added = `Added ${new Date(passkey.created_at).toLocaleDateString()}`
  const name = passkey.label || added

  const save = async () => {
    if (saving) return
    setSaving(true)
    setError(null)
    try {
      onUpdate(await updatePasskeyLabel(passkey.id, label))
      setEditing(false)
    } catch (error) {
      setError(error instanceof Error ? error.message : 'Failed to update passkey label')
    } finally {
      setSaving(false)
    }
  }

  return (
    <div className="rounded-md border p-2">
      {editing ? (
        <form className="flex flex-col gap-2" onSubmit={event => { event.preventDefault(); void save() }}>
          <Label htmlFor={`passkey-label-${passkey.id}`}>Passkey label</Label>
          <Input
            id={`passkey-label-${passkey.id}`}
            value={label}
            onChange={event => setLabel(event.target.value)}
            placeholder="e.g. Personal laptop"
            maxLength={100}
            disabled={saving}
            autoFocus
            aria-describedby={error ? `passkey-label-error-${passkey.id}` : undefined}
            onKeyDown={event => {
              if (event.key === 'Escape') {
                event.stopPropagation()
                if (!saving) setEditing(false)
              }
            }}
          />
          {error && <p id={`passkey-label-error-${passkey.id}`} role="alert" className="text-xs text-destructive">{error}</p>}
          <div className="flex justify-end gap-2">
            <Button type="button" size="sm" variant="ghost" disabled={saving} onClick={() => setEditing(false)}>Cancel</Button>
            <Button type="submit" size="sm" disabled={saving}>{saving ? 'Saving…' : 'Save label'}</Button>
          </div>
        </form>
      ) : (
        <div className="flex items-center gap-1">
          <div className="min-w-0 flex-1 text-sm">
            <div className="break-words">{name}</div>
            {passkey.label && <div className="text-xs text-muted-foreground">{added}</div>}
            <div className="text-xs text-muted-foreground">
              {passkey.last_used_at ? `Last used ${new Date(passkey.last_used_at).toLocaleDateString()}` : 'Never used'}
            </div>
          </div>
          <Button
            type="button"
            size="icon"
            variant="ghost"
            aria-label={`Edit label for ${name}`}
            onClick={() => { setLabel(passkey.label ?? ''); setError(null); setEditing(true) }}
          >
            <Pencil className="h-4 w-4" />
          </Button>
          <Button type="button" size="icon" variant="ghost" aria-label={`Remove passkey ${name}`} onClick={() => onDelete(passkey.id)}>
            <Trash2 className="h-4 w-4" />
          </Button>
        </div>
      )}
    </div>
  )
}
