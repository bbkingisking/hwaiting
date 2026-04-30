import { createContext, useContext, useState, useEffect, ReactNode } from 'react'

interface AuthContextType {
  token: string | null
  username: string | null
  login: (who: string, really: string) => Promise<{ success: boolean; error?: string }>
  logout: () => void
  isAuthenticated: boolean
}

const AuthContext = createContext<AuthContextType | undefined>(undefined)

export function AuthProvider({ children }: { children: ReactNode }) {
  const [token, setToken] = useState<string | null>(null)
  const [username, setUsername] = useState<string | null>(null)
  const [loading, setLoading] = useState(true)

  // Load token from localStorage on mount
  useEffect(() => {
    const storedToken = localStorage.getItem('annyeong-token')
    const storedUsername = localStorage.getItem('annyeong-username')
    if (storedToken && storedUsername) {
      setToken(storedToken)
      setUsername(storedUsername)
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
    localStorage.setItem('annyeong-token', data.token)
    localStorage.setItem('annyeong-username', data.username)

    return { success: true }
  }

  const logout = () => {
    setToken(null)
    setUsername(null)
    localStorage.removeItem('annyeong-token')
    localStorage.removeItem('annyeong-username')
  }

  return (
    <AuthContext.Provider
      value={{
        token,
        username,
        login,
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