import { useState } from 'react'
import { Dialog, DialogContent, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Switch } from '@/components/ui/switch'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs'
import { useAuth } from '@/components/auth-provider'

interface AuthDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
}

// Login/Sign Up (what the user is trying to do) is the primary, tab-shaped
// choice. Password/Passkey (how) is secondary and binary, so it's a Switch,
// not a second row of tabs - nesting a full tablist inside a tab panel reads
// oddly and is a shaky ARIA pattern for something that's really just an
// on/off. The method is one piece of state shared across both tabs (not
// reset when switching Login <-> Sign Up): it answers "how do I
// authenticate", which doesn't change depending on what the user is doing
// this moment.
type Method = 'password' | 'passkey'

export function AuthDialog({ open, onOpenChange }: AuthDialogProps) {
  const { login, signup, passkeyLogin, passkeyRegister } = useAuth()

  const [method, setMethod] = useState<Method>('passkey')

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
      onOpenChange(false)
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
      onOpenChange(false)
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
    if (result.success) {
      onOpenChange(false)
    } else {
      setPasskeyError(result.error || 'Sign-in failed')
    }
    setPasskeyBusy(false)
  }

  const runPasskeyRegister = async () => {
    setPasskeyError('')
    setPasskeyBusy(true)
    const result = await passkeyRegister()
    if (result.success) {
      onOpenChange(false)
    } else {
      setPasskeyError(result.error || 'Account creation failed')
    }
    setPasskeyBusy(false)
  }

  const methodToggle = (
    <div className="flex items-center justify-center gap-3 pb-2">
      <Label htmlFor="auth-method" className={method === 'password' ? '' : 'text-muted-foreground font-normal'}>
        Password
      </Label>
      <Switch
        id="auth-method"
        checked={method === 'passkey'}
        onCheckedChange={(checked) => setMethod(checked ? 'passkey' : 'password')}
      />
      <Label htmlFor="auth-method" className={method === 'passkey' ? '' : 'text-muted-foreground font-normal'}>
        Passkey
      </Label>
    </div>
  )

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>Welcome</DialogTitle>
        </DialogHeader>
        <Tabs defaultValue="signup" className="w-full">
          <TabsList className="grid w-full grid-cols-2">
            <TabsTrigger value="login">Login</TabsTrigger>
            <TabsTrigger value="signup">Sign Up</TabsTrigger>
          </TabsList>

          <TabsContent value="login" className="space-y-4">
            {methodToggle}
            {method === 'password' ? (
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
                    disabled={isLoginLoading}
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
                    disabled={isLoginLoading}
                  />
                </div>
                {loginError && (
                  <p role="alert" className="text-sm text-destructive">{loginError}</p>
                )}
                <Button type="submit" className="w-full" disabled={isLoginLoading || !loginUsername || !loginPassword}>
                  {isLoginLoading ? 'Authenticating...' : 'Enter'}
                </Button>
              </form>
            ) : (
              <div className="space-y-4">
                {passkeyError && (
                  <p role="alert" className="text-sm text-destructive">{passkeyError}</p>
                )}
                <Button onClick={runPasskeyLogin} disabled={passkeyBusy} className="w-full">
                  {passkeyBusy ? 'Waiting for passkey…' : 'Sign in'}
                </Button>
              </div>
            )}
          </TabsContent>

          <TabsContent value="signup" className="space-y-4">
            {methodToggle}
            {method === 'password' ? (
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
                    disabled={isSignupLoading}
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
                    disabled={isSignupLoading}
                  />
                </div>
                {signupError && (
                  <p role="alert" className="text-sm text-destructive">{signupError}</p>
                )}
                <Button
                  type="submit"
                  className="w-full"
                  disabled={isSignupLoading || !signupUsername || !signupPassword}
                >
                  {isSignupLoading ? 'Creating account...' : 'Sign Up'}
                </Button>
              </form>
            ) : (
              <div className="space-y-4">
                {passkeyError && (
                  <p role="alert" className="text-sm text-destructive">{passkeyError}</p>
                )}
                <Button onClick={runPasskeyRegister} disabled={passkeyBusy} className="w-full">
                  {passkeyBusy ? 'Waiting for passkey…' : 'Create account'}
                </Button>
              </div>
            )}
          </TabsContent>
        </Tabs>
      </DialogContent>
    </Dialog>
  )
}
