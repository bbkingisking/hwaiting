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

// Password and passkey are two independent ways to reach the same account
// system, both gated on the same invite codes (see backend
// auth::check_invite_code / passkey.rs's module doc) - this dialog's outer
// tabs pick the method, not "login" vs "signup" (that split lives one
// level down, inside each method, since it means something different for
// each: a password account's credentials are typed in either way, while a
// passkey sign-up registers a brand new credential and sign-in reuses one
// already on the device).
type PasskeyAction = 'login' | 'register'

export function AuthDialog({ open, onOpenChange }: AuthDialogProps) {
  const { login, signup, passkeyLogin, passkeyRegister } = useAuth()

  // Password method state
  const [loginWho, setLoginWho] = useState('')
  const [loginReally, setLoginReally] = useState('')
  const [signupWho, setSignupWho] = useState('')
  const [signupReally, setSignupReally] = useState('')
  const [signupInviteCode, setSignupInviteCode] = useState('')
  const [loginError, setLoginError] = useState('')
  const [signupError, setSignupError] = useState('')
  const [isLoginLoading, setIsLoginLoading] = useState(false)
  const [isSignupLoading, setIsSignupLoading] = useState(false)

  // Passkey method state
  const [passkeyInviteCode, setPasskeyInviteCode] = useState('')
  const [passkeyBusy, setPasskeyBusy] = useState<PasskeyAction | null>(null)
  const [passkeyError, setPasskeyError] = useState('')

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

    const result = await signup(signupWho, signupReally, signupInviteCode)

    if (result.success) {
      onOpenChange(false)
      setSignupWho('')
      setSignupReally('')
      setSignupInviteCode('')
    } else {
      setSignupError(result.error || 'Signup failed')
    }

    setIsSignupLoading(false)
  }

  const runPasskeyAction = async (action: PasskeyAction) => {
    setPasskeyError('')
    setPasskeyBusy(action)
    const result = await (action === 'login' ? passkeyLogin() : passkeyRegister(passkeyInviteCode))
    if (result.success) {
      onOpenChange(false)
      setPasskeyInviteCode('')
    } else {
      setPasskeyError(result.error || (action === 'login' ? 'Sign-in failed' : 'Account creation failed'))
    }
    setPasskeyBusy(null)
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>Welcome</DialogTitle>
        </DialogHeader>
        <Tabs defaultValue="password" className="w-full">
          <TabsList className="grid w-full grid-cols-2">
            <TabsTrigger value="password">Password</TabsTrigger>
            <TabsTrigger value="passkey">Passkey</TabsTrigger>
          </TabsList>

          <TabsContent value="password">
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
                      aria-label="Username (who?)"
                      aria-invalid={loginError !== ''}
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
                      aria-label="Password (really?)"
                      aria-invalid={loginError !== ''}
                      value={loginReally}
                      onChange={(e) => setLoginReally(e.target.value)}
                      placeholder="password"
                      autoComplete="current-password"
                      disabled={isLoginLoading}
                    />
                  </div>
                  {loginError && (
                    <p role="alert" className="text-sm text-destructive">{loginError}</p>
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
                      aria-label="Username (who?)"
                      aria-invalid={signupError !== ''}
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
                      aria-label="Password (really?)"
                      aria-invalid={signupError !== ''}
                      value={signupReally}
                      onChange={(e) => setSignupReally(e.target.value)}
                      placeholder="password"
                      autoComplete="new-password"
                      disabled={isSignupLoading}
                    />
                  </div>
                  <div className="space-y-2">
                    <Label htmlFor="signup-invite-code">invite code</Label>
                    <Input
                      id="signup-invite-code"
                      type="text"
                      aria-label="Invite code"
                      aria-invalid={signupError !== ''}
                      value={signupInviteCode}
                      onChange={(e) => setSignupInviteCode(e.target.value)}
                      placeholder="your invite code"
                      autoComplete="off"
                      disabled={isSignupLoading}
                    />
                  </div>
                  {signupError && (
                    <p role="alert" className="text-sm text-destructive">{signupError}</p>
                  )}
                  <Button
                    type="submit"
                    className="w-full"
                    disabled={isSignupLoading || !signupWho || !signupReally || !signupInviteCode}
                  >
                    {isSignupLoading ? 'Creating account...' : 'Sign Up'}
                  </Button>
                </form>
              </TabsContent>
            </Tabs>
          </TabsContent>

          <TabsContent value="passkey">
            <Tabs defaultValue="login" className="w-full">
              <TabsList className="grid w-full grid-cols-2">
                <TabsTrigger value="login">Login</TabsTrigger>
                <TabsTrigger value="signup">Sign Up</TabsTrigger>
              </TabsList>
              <TabsContent value="login" className="space-y-4">
                <p className="text-xs text-muted-foreground">
                  Sign in with a passkey already registered on this device.
                </p>
                {passkeyError && (
                  <p role="alert" className="text-sm text-destructive">{passkeyError}</p>
                )}
                <Button
                  onClick={() => runPasskeyAction('login')}
                  disabled={passkeyBusy !== null}
                  className="w-full"
                >
                  {passkeyBusy === 'login' ? 'Waiting for passkey…' : 'Sign in'}
                </Button>
              </TabsContent>
              <TabsContent value="signup" className="space-y-4">
                <div className="space-y-2">
                  <Label htmlFor="passkey-invite-code">invite code</Label>
                  <Input
                    id="passkey-invite-code"
                    type="text"
                    aria-label="Invite code"
                    aria-invalid={passkeyError !== ''}
                    value={passkeyInviteCode}
                    onChange={(e) => setPasskeyInviteCode(e.target.value)}
                    placeholder="your invite code"
                    autoComplete="off"
                    disabled={passkeyBusy !== null}
                  />
                </div>
                {passkeyError && (
                  <p role="alert" className="text-sm text-destructive">{passkeyError}</p>
                )}
                <Button
                  onClick={() => runPasskeyAction('register')}
                  disabled={passkeyBusy !== null || !passkeyInviteCode}
                  className="w-full"
                >
                  {passkeyBusy === 'register' ? 'Waiting for passkey…' : 'Create account'}
                </Button>
              </TabsContent>
            </Tabs>
          </TabsContent>
        </Tabs>
      </DialogContent>
    </Dialog>
  )
}
