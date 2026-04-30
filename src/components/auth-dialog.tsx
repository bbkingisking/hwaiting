import { useState } from 'react'
import { Dialog, DialogContent, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs'
import { useAuth } from '@/components/auth-provider'

interface AuthDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
}

export function AuthDialog({ open, onOpenChange }: AuthDialogProps) {
  const [loginWho, setLoginWho] = useState('')
  const [loginReally, setLoginReally] = useState('')
  const [signupWho, setSignupWho] = useState('')
  const [signupReally, setSignupReally] = useState('')
  const [inviteCode, setInviteCode] = useState('')
  const [loginError, setLoginError] = useState('')
  const [signupError, setSignupError] = useState('')
  const [isLoginLoading, setIsLoginLoading] = useState(false)
  const [isSignupLoading, setIsSignupLoading] = useState(false)
  const { login, signup } = useAuth()

  const handleLoginSubmit = async (e: React.FormEvent) => {
    e.preventDefault()
    setLoginError('')
    setIsLoginLoading(true)

    const result = await login(loginWho, loginReally)

    if (result.success) {
      onOpenChange(false)
      setLoginWho('')
      setLoginReally('')
    } else {
      setLoginError(result.error || 'Authentication failed')
    }

    setIsLoginLoading(false)
  }

  const handleSignupSubmit = async (e: React.FormEvent) => {
    e.preventDefault()
    setSignupError('')
    setIsSignupLoading(true)

    const result = await signup(signupWho, signupReally, inviteCode)

    if (result.success) {
      onOpenChange(false)
      setSignupWho('')
      setSignupReally('')
      setInviteCode('')
    } else {
      setSignupError(result.error || 'Signup failed')
    }

    setIsSignupLoading(false)
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>Welcome</DialogTitle>
        </DialogHeader>
        <Tabs defaultValue="login" className="w-full">
          <TabsList className="grid w-full grid-cols-2">
            <TabsTrigger value="login">Login</TabsTrigger>
            <TabsTrigger value="signup">Sign Up</TabsTrigger>
          </TabsList>
          <TabsContent value="login">
            <form onSubmit={handleLoginSubmit} className="space-y-4">
              <div className="space-y-2">
                <Label htmlFor="login-who">who?</Label>
                <Input
                  id="login-who"
                  type="text"
                  value={loginWho}
                  onChange={(e) => setLoginWho(e.target.value)}
                  placeholder="username"
                  autoComplete="username"
                  disabled={isLoginLoading}
                />
              </div>
              <div className="space-y-2">
                <Label htmlFor="login-really">really?</Label>
                <Input
                  id="login-really"
                  type="password"
                  value={loginReally}
                  onChange={(e) => setLoginReally(e.target.value)}
                  placeholder="password"
                  autoComplete="current-password"
                  disabled={isLoginLoading}
                />
              </div>
              {loginError && (
                <p className="text-sm text-destructive">{loginError}</p>
              )}
              <Button type="submit" className="w-full" disabled={isLoginLoading || !loginWho || !loginReally}>
                {isLoginLoading ? 'Authenticating...' : 'Enter'}
              </Button>
            </form>
          </TabsContent>
          <TabsContent value="signup">
            <form onSubmit={handleSignupSubmit} className="space-y-4">
              <div className="space-y-2">
                <Label htmlFor="signup-who">who?</Label>
                <Input
                  id="signup-who"
                  type="text"
                  value={signupWho}
                  onChange={(e) => setSignupWho(e.target.value)}
                  placeholder="username"
                  autoComplete="username"
                  disabled={isSignupLoading}
                />
              </div>
              <div className="space-y-2">
                <Label htmlFor="signup-really">really?</Label>
                <Input
                  id="signup-really"
                  type="password"
                  value={signupReally}
                  onChange={(e) => setSignupReally(e.target.value)}
                  placeholder="password"
                  autoComplete="new-password"
                  disabled={isSignupLoading}
                />
              </div>
              <div className="space-y-2">
                <Label htmlFor="invite-code">invite code</Label>
                <Input
                  id="invite-code"
                  type="text"
                  value={inviteCode}
                  onChange={(e) => setInviteCode(e.target.value)}
                  placeholder="your invite code"
                  autoComplete="off"
                  disabled={isSignupLoading}
                />
              </div>
              {signupError && (
                <p className="text-sm text-destructive">{signupError}</p>
              )}
              <Button type="submit" className="w-full" disabled={isSignupLoading || !signupWho || !signupReally || !inviteCode}>
                {isSignupLoading ? 'Creating account...' : 'Sign Up'}
              </Button>
            </form>
          </TabsContent>
        </Tabs>
      </DialogContent>
    </Dialog>
  )
}