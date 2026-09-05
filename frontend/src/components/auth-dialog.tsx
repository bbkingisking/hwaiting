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
// this moment. Both methods are gated on the same invite codes - see
// backend auth::check_invite_code / passkey.rs's module doc.
type Method = 'password' | 'passkey'

export function AuthDialog({ open, onOpenChange }: AuthDialogProps) {
  const { login, signup, passkeyLogin, passkeyRegister } = useAuth()

  const [method, setMethod] = useState<Method>('password')

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
  const [passkeyBusy, setPasskeyBusy] = useState(false)
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
    const result = await passkeyRegister(passkeyInviteCode)
    if (result.success) {
      onOpenChange(false)
      setPasskeyInviteCode('')
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
        <Tabs defaultValue="login" className="w-full">
          <TabsList className="grid w-full grid-cols-2">
            <TabsTrigger value="login">Login</TabsTrigger>
            <TabsTrigger value="signup">Sign Up</TabsTrigger>
          </TabsList>

          <TabsContent value="login" className="space-y-4">
            {methodToggle}
            {method === 'password' ? (
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
            ) : (
              <div className="space-y-4">
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
                    disabled={passkeyBusy}
                  />
                </div>
                {passkeyError && (
                  <p role="alert" className="text-sm text-destructive">{passkeyError}</p>
                )}
                <Button onClick={runPasskeyRegister} disabled={passkeyBusy || !passkeyInviteCode} className="w-full">
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
