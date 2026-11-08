import { connectLiveEvents } from '@/lib/live-events'
import { createContext, useContext, useState, useEffect, ReactNode } from 'react'
import { ApiError, getCapabilities, getUserProfile, logoutSession } from '@/lib/api'
import { fetchWithCookies } from '@/lib/http'
import { apiBase, assertionToJson, attestationToJson, describeCeremonyError, toCreationOptions, toRequestOptions } from '@/lib/webauthn'

interface AuthContextType {
  isAdmin: boolean
  // Whether this deployment has HWAITING_RP_ID/HWAITING_RP_ORIGINS
  // configured - see GET /api/capabilities. False (not "unknown") until
  // that fetch resolves, since AuthProvider withholds `children` until
  // `loading` clears anyway - nothing ever reads this before it's settled.
  passkeysEnabled: boolean
  // Username/password login against an existing account.
  login: (username: string, password: string) => Promise<{ success: boolean; error?: string }>
  // Username/password sign-up.
  signup: (username: string, password: string) => Promise<{ success: boolean; error?: string }>
  // Passkey sign-in: a discoverable-credential assertion against an
  // account that already exists. Takes no arguments - the passkey itself,
  // not any identifier the user types, is how the account is found.
  passkeyLogin: () => Promise<{ success: boolean; error?: string }>
  // Passkey sign-up: registers a new passkey and creates the account it
  // belongs to in the same ceremony. A deliberately separate action from
  // `passkeyLogin`, not a fallback tried after it fails - see this
  // feature's design notes for why an automatic try-then-fallback flow
  // was dropped.
  passkeyRegister: () => Promise<{ success: boolean; error?: string }>
  logout: () => Promise<{ success: boolean; error?: string }>
  isAuthenticated: boolean
}

const AuthContext = createContext<AuthContextType | undefined>(undefined)

function requireWebAuthnSupport() {
  if (!window.PublicKeyCredential) {
    throw new Error(
      'This browser has no passkey support, or this page is not a secure context (HTTPS or localhost).'
    )
  }
}

export function AuthProvider({ children }: { children: ReactNode }) {
  const [isAuthenticated, setIsAuthenticated] = useState(false)
  const [isAdmin, setIsAdmin] = useState(false)
  const [passkeysEnabled, setPasskeysEnabled] = useState(false)
  const [loading, setLoading] = useState(true)

  // Check the cookie-backed session and the server's passkey capability,
  // in parallel on mount - `loading` (and so `children`, see the provider
  // below) waits on both, so nothing ever renders with a stale/unknown
  // passkeysEnabled the way a component-local fetch could.
  useEffect(() => {
    // Leftovers from the old username/password build, which cached these
    // directly - harmless to leave, but there's no reason to.
    localStorage.removeItem('hwaiting-username')
    localStorage.removeItem('hwaiting-isadmin')
    localStorage.removeItem('hwaiting-token')

    // The HttpOnly cookie is invisible to JavaScript. A profile request
    // determines whether the browser has a valid authenticated session.
    const loadSession = async () => {
      try {
        const profile = await getUserProfile()
        setIsAdmin(profile.is_admin)
        setIsAuthenticated(true)
      } catch {
        setIsAuthenticated(false)
      }
    }

    const loadCapabilities = async () => {
      try {
        const { passkeys_enabled } = await getCapabilities()
        setPasskeysEnabled(passkeys_enabled)
      } catch {
        // Couldn't even reach the capabilities check - fail closed rather
        // than offer passkey UI that might 501.
        setPasskeysEnabled(false)
      }
    }

    Promise.all([loadSession(), loadCapabilities()]).finally(() => setLoading(false))
  }, [])

  useEffect(() => {
    if (!isAuthenticated || loading) return
    return connectLiveEvents(`${apiBase()}/api/events`, async () => {
      try { await getUserProfile(); return true }
      catch (error) {
        if (error instanceof ApiError && error.status === 401) {
          setIsAuthenticated(false)
          setIsAdmin(false)
          return false
        }
        throw error
      }
    })
  }, [isAuthenticated, loading])

  const applySession = (data: { is_admin: boolean }) => {
    setIsAuthenticated(true)
    setIsAdmin(data.is_admin)
  }

  const login = async (username: string, password: string): Promise<{ success: boolean; error?: string }> => {
    let response
    try {
      response = await fetchWithCookies(`${apiBase()}/api/auth/login`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ username, password }),
      })
    } catch (error) {
      return { success: false, error: 'Network error - could not connect to server' }
    }

    const data = await response.json().catch(() => ({}))
    if (!response.ok) {
      return { success: false, error: data.error || 'Authentication failed' }
    }
    applySession(data)
    return { success: true }
  }

  const signup = async (
    username: string,
    password: string
  ): Promise<{ success: boolean; error?: string }> => {
    let response
    try {
      response = await fetchWithCookies(`${apiBase()}/api/auth/signup`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ username, password }),
      })
    } catch (error) {
      return { success: false, error: 'Network error - could not connect to server' }
    }

    const data = await response.json().catch(() => ({}))
    if (!response.ok) {
      return { success: false, error: data.error || 'Signup failed' }
    }
    applySession(data)
    return { success: true }
  }

  // Shared shape of both passkey ceremonies: POST start (with `startBody`,
  // if any), run the matching navigator.credentials.* call, POST finish
  // with the result. `runCeremony` is async so callers still get one
  // `{ success, error }` result whether the failure came from the network,
  // the browser, or the server.
  const runCeremony = async (
    kind: 'login' | 'register',
    startBody: object | undefined,
    getCredential: (publicKey: any) => Promise<PublicKeyCredential | null>,
    toJson: (cred: PublicKeyCredential) => unknown
  ): Promise<{ success: boolean; error?: string }> => {
    try {
      requireWebAuthnSupport()

      const startResponse = await fetchWithCookies(`${apiBase()}/api/auth/passkey/${kind}/start`, {
        method: 'POST',
        ...(startBody && {
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify(startBody),
        }),
      })
      const startData = await startResponse.json().catch(() => ({}))
      if (!startResponse.ok) {
        return { success: false, error: startData.error || `Could not start ${kind}` }
      }
      const { ceremony_id, options } = startData

      let credential: PublicKeyCredential | null
      try {
        credential = await getCredential(options)
      } catch (e) {
        return { success: false, error: describeCeremonyError(e, kind === 'login' ? 'Sign-in' : 'Account creation') }
      }
      if (!credential) {
        return { success: false, error: `${kind === 'login' ? 'Sign-in' : 'Account creation'} was cancelled.` }
      }

      const finishResponse = await fetchWithCookies(`${apiBase()}/api/auth/passkey/${kind}/finish`, {
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

  const passkeyLogin = () =>
    runCeremony(
      'login',
      undefined,
      (publicKey) => navigator.credentials.get({ publicKey: toRequestOptions(publicKey) }) as Promise<PublicKeyCredential | null>,
      assertionToJson
    )

  const passkeyRegister = () =>
    runCeremony(
      'register',
      undefined,
      (publicKey) => navigator.credentials.create({ publicKey: toCreationOptions(publicKey) }) as Promise<PublicKeyCredential | null>,
      attestationToJson
    )

  const logout = async (): Promise<{ success: boolean; error?: string }> => {
    try {
      await logoutSession()
      setIsAuthenticated(false)
      setIsAdmin(false)
      return { success: true }
    } catch {
      return { success: false, error: 'Could not log out. Please try again.' }
    }
  }

  return (
    <AuthContext.Provider
      value={{
        isAdmin,
        passkeysEnabled,
        login,
        signup,
        passkeyLogin,
        passkeyRegister,
        logout,
        isAuthenticated: isAuthenticated && !loading,
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
