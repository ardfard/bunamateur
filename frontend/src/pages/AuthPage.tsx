import React, { useEffect, useState } from 'react'
import { Lock, Mail, Moon, ShieldCheck, Sun, User } from 'lucide-react'
import { api } from '../api'
import type { AuthUser } from '../types'

interface AuthPageProps {
  darkMode: boolean
  onToggleDark: () => void
  onAuthenticated: (user: AuthUser) => void
}

const OAUTH_ERROR_MESSAGES: Record<string, string> = {
  google_not_configured:
    'Google Sign-In is not configured on this server yet. Set GOOGLE_CLIENT_ID and GOOGLE_CLIENT_SECRET.',
  google_cancelled: 'Google Sign-In was cancelled.',
  invalid_state: 'Google Sign-In session expired or state mismatch. Please try again.',
  google_failed: 'Google Sign-In failed. Please try again or use email login.',
}

export const AuthPage: React.FC<AuthPageProps> = ({
  darkMode,
  onToggleDark,
  onAuthenticated,
}) => {
  const [mode, setMode] = useState<'login' | 'register'>('login')
  const [email, setEmail] = useState('')
  const [username, setUsername] = useState('')
  const [password, setPassword] = useState('')
  const [busy, setBusy] = useState(false)
  const [googleEnabled, setGoogleEnabled] = useState<boolean | null>(null)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    api
      .authProviders()
      .then((res) => setGoogleEnabled(res.google))
      .catch(() => setGoogleEnabled(false))

    if (typeof window !== 'undefined') {
      const params = new URLSearchParams(window.location.search)
      const authErr = params.get('auth_error')
      if (authErr) {
        setError(OAUTH_ERROR_MESSAGES[authErr] || 'Sign-in failed. Please try again.')
        params.delete('auth_error')
        const clean = params.toString()
        const nextUrl = `${window.location.pathname}${clean ? `?${clean}` : ''}`
        window.history.replaceState({}, '', nextUrl)
      }
    }
  }, [])

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault()
    setBusy(true)
    setError(null)
    try {
      const resp =
        mode === 'register'
          ? await api.register({ email: email.trim(), username: username.trim(), password })
          : await api.login({ email: email.trim(), password })
      onAuthenticated(resp.user)
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Authentication failed')
    } finally {
      setBusy(false)
    }
  }

  const handleGoogleLogin = () => {
    if (googleEnabled === false) {
      setError(OAUTH_ERROR_MESSAGES.google_not_configured)
      return
    }
    window.location.href = '/api/auth/google/start'
  }

  return (
    <div
      className="min-h-screen flex flex-col items-center justify-center px-4 py-10"
      style={{ backgroundColor: 'var(--bg-canvas)' }}
    >
      <div className="fixed top-4 right-4">
        <button
          onClick={onToggleDark}
          className="flex h-9 w-9 items-center justify-center rounded-lg border transition-colors cursor-pointer"
          style={{
            borderColor: 'var(--border-subtle)',
            backgroundColor: 'var(--bg-surface)',
            color: 'var(--text-secondary)',
          }}
          title={darkMode ? 'Switch to Washi Light Mode' : 'Switch to Sumi Dark Mode'}
        >
          {darkMode ? <Sun className="h-4 w-4" /> : <Moon className="h-4 w-4" />}
        </button>
      </div>

      <div
        className="w-full max-w-[420px] rounded-2xl border p-7 shadow-sm"
        style={{
          backgroundColor: 'var(--bg-surface)',
          borderColor: 'var(--border-subtle)',
        }}
      >
        <div className="flex items-center gap-3 mb-6">
          <div
            className="flex h-10 w-10 items-center justify-center rounded-xl font-jp text-xl font-bold text-white shadow-xs"
            style={{ backgroundColor: 'var(--accent-vermilion)' }}
          >
            文
          </div>
          <div>
            <h1 className="text-xl font-bold leading-tight tracking-tight">BunAmateur</h1>
            <p className="font-mono-code text-[11px] uppercase tracking-wider" style={{ color: 'var(--text-muted)' }}>
              文アマ • Japanese Grammar SRS
            </p>
          </div>
        </div>

        <div
          className="grid grid-cols-2 gap-1 rounded-xl p-1 mb-5"
          style={{ backgroundColor: 'var(--bg-subtle)' }}
        >
          <button
            type="button"
            onClick={() => {
              setMode('login')
              setError(null)
            }}
            className="rounded-lg py-2 text-xs font-semibold transition-colors cursor-pointer"
            style={{
              backgroundColor: mode === 'login' ? 'var(--bg-surface)' : 'transparent',
              color: mode === 'login' ? 'var(--text-primary)' : 'var(--text-secondary)',
            }}
          >
            Sign In
          </button>
          <button
            type="button"
            onClick={() => {
              setMode('register')
              setError(null)
            }}
            className="rounded-lg py-2 text-xs font-semibold transition-colors cursor-pointer"
            style={{
              backgroundColor: mode === 'register' ? 'var(--bg-surface)' : 'transparent',
              color: mode === 'register' ? 'var(--text-primary)' : 'var(--text-secondary)',
            }}
          >
            Create Account
          </button>
        </div>

        {error && (
          <div
            className="mb-4 rounded-xl border px-3.5 py-2.5 text-xs font-medium"
            style={{
              backgroundColor: 'var(--accent-vermilion-soft)',
              borderColor: 'var(--accent-vermilion)',
              color: 'var(--accent-vermilion)',
            }}
          >
            {error}
          </div>
        )}

        {/* Continue with Google (OAuth 2.0 + PKCE S256) */}
        <button
          type="button"
          onClick={handleGoogleLogin}
          disabled={busy}
          className="flex w-full items-center justify-center gap-2.5 rounded-xl border py-2.5 text-sm font-semibold transition-colors hover:opacity-90 disabled:opacity-50 cursor-pointer"
          style={{
            backgroundColor: 'var(--bg-canvas)',
            borderColor: 'var(--border-subtle)',
            color: 'var(--text-primary)',
          }}
        >
          <svg className="h-4 w-4" viewBox="0 0 24 24" aria-hidden="true">
            <path
              fill="#4285F4"
              d="M23.49 12.27c0-.79-.07-1.54-.2-2.27H12v4.51h6.45a5.52 5.52 0 0 1-2.39 3.62v3h3.87c2.26-2.08 3.56-5.15 3.56-8.86z"
            />
            <path
              fill="#34A853"
              d="M12 24c3.24 0 5.95-1.07 7.93-2.9l-3.87-3c-1.07.72-2.45 1.15-4.06 1.15-3.12 0-5.77-2.11-6.71-4.95H1.29v3.1A12 12 0 0 0 12 24z"
            />
            <path
              fill="#FBBC05"
              d="M5.29 14.3a7.2 7.2 0 0 1 0-4.6V6.6H1.29a12 12 0 0 0 0 10.8l4-3.1z"
            />
            <path
              fill="#EA4335"
              d="M12 4.75c1.76 0 3.34.61 4.58 1.8l3.44-3.44C17.95 1.19 15.24 0 12 0A12 12 0 0 0 1.29 6.6l4 3.1c.94-2.84 3.59-4.95 6.71-4.95z"
            />
          </svg>
          <span>Continue with Google</span>
        </button>

        <div className="my-4 flex items-center gap-3">
          <div className="h-px flex-1" style={{ backgroundColor: 'var(--border-subtle)' }} />
          <span className="text-[11px] font-medium uppercase tracking-wider" style={{ color: 'var(--text-muted)' }}>
            or email
          </span>
          <div className="h-px flex-1" style={{ backgroundColor: 'var(--border-subtle)' }} />
        </div>

        <form onSubmit={handleSubmit} className="flex flex-col gap-3.5">
          <div>
            <label className="mb-1.5 block text-xs font-semibold" style={{ color: 'var(--text-secondary)' }}>
              Email
            </label>
            <div
              className="flex items-center gap-2.5 rounded-xl border px-3.5 py-2.5"
              style={{
                backgroundColor: 'var(--bg-canvas)',
                borderColor: 'var(--border-subtle)',
              }}
            >
              <Mail className="h-4 w-4 shrink-0" style={{ color: 'var(--text-muted)' }} />
              <input
                type="email"
                required
                autoComplete="email"
                value={email}
                onChange={(e) => setEmail(e.target.value)}
                placeholder="you@example.com"
                className="w-full bg-transparent text-sm outline-none"
                style={{ color: 'var(--text-primary)' }}
              />
            </div>
          </div>

          {mode === 'register' && (
            <div>
              <label className="mb-1.5 block text-xs font-semibold" style={{ color: 'var(--text-secondary)' }}>
                Username
              </label>
              <div
                className="flex items-center gap-2.5 rounded-xl border px-3.5 py-2.5"
                style={{
                  backgroundColor: 'var(--bg-canvas)',
                  borderColor: 'var(--border-subtle)',
                }}
              >
                <User className="h-4 w-4 shrink-0" style={{ color: 'var(--text-muted)' }} />
                <input
                  type="text"
                  required
                  minLength={2}
                  maxLength={32}
                  autoComplete="username"
                  value={username}
                  onChange={(e) => setUsername(e.target.value)}
                  placeholder="akira_jp"
                  className="w-full bg-transparent text-sm outline-none"
                  style={{ color: 'var(--text-primary)' }}
                />
              </div>
            </div>
          )}

          <div>
            <label className="mb-1.5 block text-xs font-semibold" style={{ color: 'var(--text-secondary)' }}>
              Password
            </label>
            <div
              className="flex items-center gap-2.5 rounded-xl border px-3.5 py-2.5"
              style={{
                backgroundColor: 'var(--bg-canvas)',
                borderColor: 'var(--border-subtle)',
              }}
            >
              <Lock className="h-4 w-4 shrink-0" style={{ color: 'var(--text-muted)' }} />
              <input
                type="password"
                required
                minLength={8}
                autoComplete={mode === 'register' ? 'new-password' : 'current-password'}
                value={password}
                onChange={(e) => setPassword(e.target.value)}
                placeholder="At least 8 characters"
                className="w-full bg-transparent text-sm outline-none"
                style={{ color: 'var(--text-primary)' }}
              />
            </div>
          </div>

          <button
            type="submit"
            disabled={busy}
            className="mt-2 rounded-xl py-2.5 text-sm font-semibold text-white transition-opacity hover:opacity-90 disabled:opacity-50 cursor-pointer"
            style={{ backgroundColor: 'var(--accent-vermilion)' }}
          >
            {busy
              ? mode === 'register'
                ? 'Creating account...'
                : 'Signing in...'
              : mode === 'register'
                ? 'Create Account'
                : 'Sign In'}
          </button>
        </form>

        <div
          className="mt-5 flex items-center gap-2 rounded-xl px-3 py-2 text-[11px]"
          style={{
            backgroundColor: 'var(--bg-subtle)',
            color: 'var(--text-muted)',
          }}
        >
          <ShieldCheck className="h-3.5 w-3.5 shrink-0" style={{ color: 'var(--accent-matcha)' }} />
          <span>Google OAuth (PKCE), Argon2id passwords, HTTP-only sessions, CSRF &amp; AES-256-GCM WaniKani encryption.</span>
        </div>
      </div>
    </div>
  )
}
