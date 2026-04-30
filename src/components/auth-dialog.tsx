import { useState } from 'react'
import { Dialog, DialogContent, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { useAuth } from '@/components/auth-provider'

interface AuthDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
}

export function AuthDialog({ open, onOpenChange }: AuthDialogProps) {
  const [who, setWho] = useState('')
  const [really, setReally] = useState('')
  const [error, setError] = useState('')
  const [isLoading, setIsLoading] = useState(false)
  const { login } = useAuth()

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault()
    setError('')
    setIsLoading(true)

    const result = await login(who, really)

    if (result.success) {
      onOpenChange(false)
      setWho('')
      setReally('')
    } else {
      setError(result.error || 'Authentication failed')
    }

    setIsLoading(false)
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>Welcome</DialogTitle>
        </DialogHeader>
        <form onSubmit={handleSubmit} className="space-y-4">
          <div className="space-y-2">
            <Label htmlFor="who">who?</Label>
            <Input
              id="who"
              type="text"
              value={who}
              onChange={(e) => setWho(e.target.value)}
              placeholder="username"
              autoComplete="username"
              autoFocus
              disabled={isLoading}
            />
          </div>
          <div className="space-y-2">
            <Label htmlFor="really">really?</Label>
            <Input
              id="really"
              type="password"
              value={really}
              onChange={(e) => setReally(e.target.value)}
              placeholder="password"
              autoComplete="current-password"
              disabled={isLoading}
            />
          </div>
          {error && (
            <p className="text-sm text-destructive">{error}</p>
          )}
          <Button type="submit" className="w-full" disabled={isLoading || !who || !really}>
            {isLoading ? 'Authenticating...' : 'Enter'}
          </Button>
        </form>
      </DialogContent>
    </Dialog>
  )
}