import { useState } from 'react'
import { Dialog, DialogContent, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs'
import { useAuth } from '@/components/auth-provider'

export function AuthDialog() {
  const { isAuthenticated, login, signup, passkeyLogin, passkeyRegister, passkeysEnabled, signupsEnabled } = useAuth()

  const [activeTab, setActiveTab] = useState('login')

  // Password method state
  const [loginUsername, setLoginUsername] = useState('')
  const [loginPassword, setLoginPassword] = useState('')
  const [signupUsername, setSignupUsername] = useState('')
  const [signupPassword, setSignupPassword] = useState('')
  const [loginError, setLoginError] = useState('')
  const [signupError, setSignupError] = useState('')
  const [isLoginLoading, setIsLoginLoading] = useState(false)
  const [isSignupLoading, setIsSignupLoading] = useState(false)

  // Passkey method state
  const [passkeyBusy, setPasskeyBusy] = useState(false)
  const [passkeyError, setPasskeyError] = useState('')

  const handleLoginSubmit = async (e: React.FormEvent) => {
    e.preventDefault()
    setLoginError('')
    setIsLoginLoading(true)

    const result = await login(loginUsername, loginPassword)

    if (result.success) {
      setLoginUsername('')
      setLoginPassword('')
    } else {
      setLoginError(result.error || 'Authentication failed')
    }

    setIsLoginLoading(false)
  }

  const handleSignupSubmit = async (e: React.FormEvent) => {
    e.preventDefault()
    setSignupError('')
    setIsSignupLoading(true)

    const result = await signup(signupUsername, signupPassword)

    if (result.success) {
      setSignupUsername('')
      setSignupPassword('')
    } else {
      setSignupError(result.error || 'Signup failed')
    }

    setIsSignupLoading(false)
  }

  const runPasskeyLogin = async () => {
    setPasskeyError('')
    setPasskeyBusy(true)
    const result = await passkeyLogin()
    if (!result.success) {
      setPasskeyError(result.error || 'Sign-in failed')
    }
    setPasskeyBusy(false)
  }

  const runPasskeyRegister = async () => {
    setPasskeyError('')
    setPasskeyBusy(true)
    const result = await passkeyRegister()
    if (!result.success) {
      setPasskeyError(result.error || 'Account creation failed')
    }
    setPasskeyBusy(false)
  }

  return (
    <Dialog
      open={!isAuthenticated}
      disablePointerDismissal
      onOpenChange={(_, eventDetails) => {
        // Authentication is required; only a session change can close this dialog.
        eventDetails.cancel()
      }}
    >
      <DialogContent className="sm:max-w-md" showCloseButton={false}>
        <DialogHeader>
          <DialogTitle lang="ko" className="text-center">어서 오세요</DialogTitle>
        </DialogHeader>
        <Tabs
          value={activeTab}
          className="w-full"
          onValueChange={(value) => {
            if (value === null || passkeyBusy || isLoginLoading || isSignupLoading) return
            setActiveTab(value as string)
            setLoginError('')
            setSignupError('')
            setPasskeyError('')
          }}
        >
          {signupsEnabled && <TabsList className="grid w-full grid-cols-2">
            <TabsTrigger value="login" disabled={passkeyBusy || isLoginLoading || isSignupLoading}>Log in</TabsTrigger>
            <TabsTrigger value="signup" disabled={passkeyBusy || isLoginLoading || isSignupLoading}>Sign up</TabsTrigger>
          </TabsList>}

          <TabsContent value="login" className="space-y-4">
            <form onSubmit={handleLoginSubmit} className="space-y-4">
              <div className="space-y-2">
                <Label htmlFor="login-username">Username</Label>
                <Input
                  id="login-username"
                  type="text"
                  aria-invalid={loginError !== ''}
                  value={loginUsername}
                  onChange={(e) => setLoginUsername(e.target.value)}
                  placeholder="username"
                  autoComplete="username"
                  required
                  disabled={isLoginLoading || passkeyBusy}
                />
              </div>
              <div className="space-y-2">
                <Label htmlFor="login-password">Password</Label>
                <Input
                  id="login-password"
                  type="password"
                  aria-invalid={loginError !== ''}
                  value={loginPassword}
                  onChange={(e) => setLoginPassword(e.target.value)}
                  placeholder="password"
                  autoComplete="current-password"
                  required
                  disabled={isLoginLoading || passkeyBusy}
                />
              </div>
              {loginError && (
                <p role="alert" className="text-sm text-destructive">{loginError}</p>
              )}
              <Button type="submit" className="w-full" disabled={isLoginLoading || passkeyBusy || !loginUsername.trim() || !loginPassword.trim()}>
                {isLoginLoading ? 'Authenticating...' : 'Log in'}
              </Button>
            </form>
            {passkeysEnabled && (
              <div className="space-y-4">
                <div className="flex items-center gap-3 text-xs text-muted-foreground">
                  <div className="h-px flex-1 bg-border" />
                  <span>OR</span>
                  <div className="h-px flex-1 bg-border" />
                </div>
                {passkeyError && (
                  <p role="alert" className="text-sm text-destructive">{passkeyError}</p>
                )}
                <Button onClick={runPasskeyLogin} disabled={passkeyBusy || isLoginLoading || isSignupLoading} variant="outline" className="w-full">
                  {passkeyBusy ? 'Waiting for passkey…' : 'Continue with a passkey'}
                </Button>
              </div>
            )}
          </TabsContent>

          {signupsEnabled && <TabsContent value="signup" className="space-y-4">
            <form onSubmit={handleSignupSubmit} className="space-y-4">
              <div className="space-y-2">
                <Label htmlFor="signup-username">Username</Label>
                <Input
                  id="signup-username"
                  type="text"
                  aria-invalid={signupError !== ''}
                  value={signupUsername}
                  onChange={(e) => setSignupUsername(e.target.value)}
                  placeholder="username"
                  autoComplete="username"
                  required
                  disabled={isSignupLoading || passkeyBusy}
                />
              </div>
              <div className="space-y-2">
                <Label htmlFor="signup-password">Password</Label>
                <Input
                  id="signup-password"
                  type="password"
                  aria-invalid={signupError !== ''}
                  value={signupPassword}
                  onChange={(e) => setSignupPassword(e.target.value)}
                  placeholder="password"
                  autoComplete="new-password"
                  required
                  disabled={isSignupLoading || passkeyBusy}
                />
              </div>
              {signupError && (
                <p role="alert" className="text-sm text-destructive">{signupError}</p>
              )}
              <Button
                type="submit"
                className="w-full"
                disabled={isSignupLoading || passkeyBusy || !signupUsername.trim() || !signupPassword.trim()}
              >
                {isSignupLoading ? 'Creating account...' : 'Sign up'}
              </Button>
            </form>
            {passkeysEnabled && (
              <div className="space-y-4">
                <div className="flex items-center gap-3 text-xs text-muted-foreground">
                  <div className="h-px flex-1 bg-border" />
                  <span>OR</span>
                  <div className="h-px flex-1 bg-border" />
                </div>
                {passkeyError && (
                  <p role="alert" className="text-sm text-destructive">{passkeyError}</p>
                )}
                <Button onClick={runPasskeyRegister} disabled={passkeyBusy || isLoginLoading || isSignupLoading} variant="outline" className="w-full">
                  {passkeyBusy ? 'Waiting for passkey…' : 'Create an account with a passkey'}
                </Button>
              </div>
            )}
          </TabsContent>}
        </Tabs>
      </DialogContent>
    </Dialog>
  )
}
