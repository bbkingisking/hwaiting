import { createContext, useContext, useState, useEffect, ReactNode } from 'react'

interface AuthContextType {
  token: string | null
  username: string | null
  isAdmin: boolean
  login: (who: string, really: string) => Promise<{ success: boolean; error?: string }>
  signup: (who: string, really: string, inviteCode: string) => Promise<{ success: boolean; error?: string }>
  logout: () => void
  isAuthenticated: boolean
}

const AuthContext = createContext<AuthContextType | undefined>(undefined)

export function AuthProvider({ children }: { children: ReactNode }) {
  const [token, setToken] = useState<string | null>(null)
  const [username, setUsername] = useState<string | null>(null)
  const [isAdmin, setIsAdmin] = useState(false)
  const [loading, setLoading] = useState(true)

  // Load token from localStorage on mount
  useEffect(() => {
    const storedToken = localStorage.getItem('annyeong-token')
    const storedUsername = localStorage.getItem('annyeong-username')
    if (storedToken && storedUsername) {
      setToken(storedToken)
      setUsername(storedUsername)
      // Always check username to determine admin status
      setIsAdmin(storedUsername === 'seok')
      localStorage.setItem('annyeong-isadmin', storedUsername === 'seok' ? 'true' : 'false')
    }
    setLoading(false)
  }, [])

  const login = async (who: string, really: string): Promise<{ success: boolean; error?: string }> => {
    let response
    try {
      const url = `${window.location.origin}/api/auth/login`
      
      response = await fetch(url, {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
        },
        body: JSON.stringify({ who, really }),
      })
    } catch (error) {
      return { success: false, error: 'Network error - could not connect to server' }
    }

    // Parse JSON response
    let data
    try {
      data = await response.json()
    } catch (e) {
      return { success: false, error: 'Invalid response from server' }
    }

    if (!response.ok) {
      return { success: false, error: data.error || 'Authentication failed' }
    }
    setToken(data.token)
    setUsername(data.username)
    setIsAdmin(data.username === 'seok')
    localStorage.setItem('annyeong-token', data.token)
    localStorage.setItem('annyeong-username', data.username)
    localStorage.setItem('annyeong-isadmin', data.username === 'seok' ? 'true' : 'false')

    return { success: true }
  }

  const signup = async (who: string, really: string, inviteCode: string): Promise<{ success: boolean; error?: string }> => {
    let response
    try {
      const url = `${window.location.origin}/api/auth/signup`
      
      response = await fetch(url, {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
        },
        body: JSON.stringify({ who, really, invite_code: inviteCode }),
      })
    } catch (error) {
      return { success: false, error: 'Network error - could not connect to server' }
    }

    // Parse JSON response
    let data
    try {
      data = await response.json()
    } catch (e) {
      return { success: false, error: 'Invalid response from server' }
    }

    if (!response.ok) {
      return { success: false, error: data.error || 'Signup failed' }
    }
    setToken(data.token)
    setUsername(data.username)
    setIsAdmin(false)
    localStorage.setItem('annyeong-token', data.token)
    localStorage.setItem('annyeong-username', data.username)
    localStorage.setItem('annyeong-isadmin', 'false')

    return { success: true }
  }

  const logout = () => {
    setToken(null)
    setUsername(null)
    setIsAdmin(false)
    localStorage.removeItem('annyeong-token')
    localStorage.removeItem('annyeong-username')
    localStorage.removeItem('annyeong-isadmin')
  }

  return (
    <AuthContext.Provider
      value={{
        token,
        username,
        isAdmin,
        login,
        signup,
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