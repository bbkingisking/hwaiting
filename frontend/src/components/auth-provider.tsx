import { createContext, useContext, useState, useEffect, ReactNode } from 'react'
import { getUserProfile } from '@/lib/api'
import { apiBase, assertionToJson, attestationToJson, describeCeremonyError, toCreationOptions, toRequestOptions } from '@/lib/webauthn'

interface AuthContextType {
  token: string | null
  isAdmin: boolean
  // Passkey sign-in: a discoverable-credential assertion against an
  // account that already exists. Takes no arguments - see [[hwaiting
  // passkey plan]] for why: no username, email, or any other identifier is
  // ever collected.
  login: () => Promise<{ success: boolean; error?: string }>
  // Passkey sign-up: registers a new passkey and creates the account it
  // belongs to in the same ceremony. A deliberately separate action from
  // `login`, not a fallback tried after it fails - see this feature's
  // design notes for why an automatic try-then-fallback flow was dropped.
  register: () => Promise<{ success: boolean; error?: string }>
  logout: () => void
  isAuthenticated: boolean
}

const AuthContext = createContext<AuthContextType | undefined>(undefined)

const TOKEN_KEY = 'annyeong-token'

function requireWebAuthnSupport() {
  if (!window.PublicKeyCredential) {
    throw new Error(
      'This browser has no passkey support, or this page is not a secure context (HTTPS or localhost).'
    )
  }
}

export function AuthProvider({ children }: { children: ReactNode }) {
  const [token, setToken] = useState<string | null>(null)
  const [isAdmin, setIsAdmin] = useState(false)
  const [loading, setLoading] = useState(true)

  // Load the token from localStorage on mount. The JWT itself carries no
  // is_admin claim (see backend auth::Claims), so that has to come from a
  // profile fetch rather than being cached alongside the token.
  useEffect(() => {
    // Leftovers from the old username/password build, which cached these
    // directly - harmless to leave, but there's no reason to.
    localStorage.removeItem('annyeong-username')
    localStorage.removeItem('annyeong-isadmin')

    const storedToken = localStorage.getItem(TOKEN_KEY)
    if (!storedToken) {
      setLoading(false)
      return
    }

    setToken(storedToken)
    getUserProfile()
      .then((profile) => setIsAdmin(profile.is_admin))
      .catch(() => {
        // Token rejected or expired - don't stay "authenticated" with a
        // token that doesn't actually work.
        localStorage.removeItem(TOKEN_KEY)
        setToken(null)
      })
      .finally(() => setLoading(false))
  }, [])

  const applySession = (data: { token: string; is_admin: boolean }) => {
    setToken(data.token)
    setIsAdmin(data.is_admin)
    localStorage.setItem(TOKEN_KEY, data.token)
  }

  // Shared shape of both ceremonies: POST start (no body), run the
  // matching navigator.credentials.* call, POST finish with the result.
  // `runCeremony` and `finish` are async so callers still get one
  // `{ success, error }` result whether the failure came from the network,
  // the browser, or the server.
  const runCeremony = async (
    kind: 'login' | 'register',
    getCredential: (publicKey: any) => Promise<PublicKeyCredential | null>,
    toJson: (cred: PublicKeyCredential) => unknown
  ): Promise<{ success: boolean; error?: string }> => {
    try {
      requireWebAuthnSupport()

      const startResponse = await fetch(`${apiBase()}/api/auth/passkey/${kind}/start`, { method: 'POST' })
      const startData = await startResponse.json().catch(() => ({}))
      if (!startResponse.ok) {
        return { success: false, error: startData.error || `Could not start ${kind}` }
      }
      const { ceremony_id, options } = startData

      let credential: PublicKeyCredential | null
      try {
        credential = await getCredential(options.publicKey)
      } catch (e) {
        return { success: false, error: describeCeremonyError(e, kind === 'login' ? 'Sign-in' : 'Account creation') }
      }
      if (!credential) {
        return { success: false, error: `${kind === 'login' ? 'Sign-in' : 'Account creation'} was cancelled.` }
      }

      const finishResponse = await fetch(`${apiBase()}/api/auth/passkey/${kind}/finish`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ ceremony_id, credential: toJson(credential) }),
      })
      const finishData = await finishResponse.json().catch(() => ({}))
      if (!finishResponse.ok) {
        return { success: false, error: finishData.error || `${kind === 'login' ? 'Sign-in' : 'Account creation'} failed` }
      }

      applySession(finishData)
      return { success: true }
    } catch (error) {
      return { success: false, error: error instanceof Error ? error.message : 'Network error - could not connect to server' }
    }
  }

  const login = () =>
    runCeremony(
      'login',
      (publicKey) => navigator.credentials.get({ publicKey: toRequestOptions(publicKey) }) as Promise<PublicKeyCredential | null>,
      assertionToJson
    )

  const register = () =>
    runCeremony(
      'register',
      (publicKey) => navigator.credentials.create({ publicKey: toCreationOptions(publicKey) }) as Promise<PublicKeyCredential | null>,
      attestationToJson
    )

  const logout = () => {
    setToken(null)
    setIsAdmin(false)
    localStorage.removeItem(TOKEN_KEY)
  }

  return (
    <AuthContext.Provider
      value={{
        token,
        isAdmin,
        login,
        register,
        logout,
        isAuthenticated: !!token && !loading,
      }}
    >
      {loading ? null : children}
    </AuthContext.Provider>
  )
}

export function useAuth() {
  const context = useContext(AuthContext)
  if (context === undefined) {
    throw new Error('useAuth must be used within an AuthProvider')
  }
  return context
}
