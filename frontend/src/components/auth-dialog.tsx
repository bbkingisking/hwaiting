import { useState } from 'react'
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { Button } from '@/components/ui/button'
import { useAuth } from '@/components/auth-provider'

interface AuthDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
}

type Action = 'login' | 'register'

export function AuthDialog({ open, onOpenChange }: AuthDialogProps) {
  const { login, register } = useAuth()
  const [busy, setBusy] = useState<Action | null>(null)
  const [error, setError] = useState('')

  const run = async (action: Action) => {
    setError('')
    setBusy(action)
    const result = await (action === 'login' ? login() : register())
    if (result.success) {
      onOpenChange(false)
    } else {
      setError(result.error || (action === 'login' ? 'Sign-in failed' : 'Account creation failed'))
    }
    setBusy(null)
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>Welcome</DialogTitle>
          <DialogDescription>
            No username, email, or password - just a passkey on this device.
          </DialogDescription>
        </DialogHeader>
        <div className="flex flex-col gap-3">
          <Button onClick={() => run('login')} disabled={busy !== null} className="w-full">
            {busy === 'login' ? 'Waiting for passkey…' : 'Sign in'}
          </Button>
          <Button onClick={() => run('register')} disabled={busy !== null} variant="outline" className="w-full">
            {busy === 'register' ? 'Waiting for passkey…' : 'Create account'}
          </Button>
          {error && (
            <p role="alert" className="text-sm text-destructive">{error}</p>
          )}
        </div>
      </DialogContent>
    </Dialog>
  )
}
