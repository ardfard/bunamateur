import React, { useState } from 'react'
import { Lock, Mail, Moon, ShieldCheck, Sun, User } from 'lucide-react'
import { api } from '../api'
import type { AuthUser } from '../types'

interface AuthPageProps {
  darkMode: boolean
  onToggleDark: () => void
  onAuthenticated: (user: AuthUser) => void
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
  const [error, setError] = useState<string | null>(null)

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
          className="grid grid-cols-2 gap-1 rounded-xl p-1 mb-6"
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
          <span>Argon2id passwords, HTTP-only session cookies, CSRF protection & AES-256-GCM WaniKani token encryption.</span>
        </div>
      </div>
    </div>
  )
}
