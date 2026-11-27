import React, { useCallback, useEffect, useMemo, useState } from 'react'
import {
  CheckCircle2,
  Compass,
  KeyRound,
  Languages,
  Laptop,
  Lock,
  LogOut,
  Mail,
  Monitor,
  Shield,
  ShieldCheck,
  Smartphone,
  Trash2,
  User,
  X,
} from 'lucide-react'
import { api } from '../api'
import { WaniKaniPage } from '../pages/WaniKaniPage'
import type {
  AccountProfileResponse,
  AccountSessionInfo,
  AuthUser,
  WaniKaniStatus,
} from '../types'
import { useToast } from './ToastContext'

export type SettingsTab = 'profile' | 'password' | 'sessions' | 'wanikani'

interface AccountSettingsModalProps {
  open: boolean
  initialTab?: SettingsTab
  onClose: () => void
  onUserUpdated: (user: AuthUser) => void
  wanikaniStatus?: WaniKaniStatus | null
  knownKanji?: Set<string>
  onWaniKaniUpdated?: () => void
  onRestartOnboarding?: () => Promise<void> | void
}

function parseUserAgent(ua: string | null): { label: string; isMobile: boolean } {
  if (!ua || !ua.trim()) {
    return { label: 'Unknown device', isMobile: false }
  }

  let browser = 'Browser'
  if (/Edg\//i.test(ua)) browser = 'Edge'
  else if (/OPR\/|Opera/i.test(ua)) browser = 'Opera'
  else if (/Firefox\//i.test(ua)) browser = 'Firefox'
  else if (/Chrome\//i.test(ua) && !/Chromium/i.test(ua)) browser = 'Chrome'
  else if (/Safari\//i.test(ua) && !/Chrome\//i.test(ua)) browser = 'Safari'
  else if (/curl\//i.test(ua)) browser = 'curl'

  let os = ''
  let isMobile = false
  if (/iPhone|iPad|iPod/i.test(ua)) {
    os = /iPad/i.test(ua) ? 'iPadOS' : 'iOS'
    isMobile = true
  } else if (/Android/i.test(ua)) {
    os = 'Android'
    isMobile = true
  } else if (/Mac OS X|Macintosh/i.test(ua)) {
    os = 'macOS'
  } else if (/Windows/i.test(ua)) {
    os = 'Windows'
  } else if (/Linux/i.test(ua)) {
    os = 'Linux'
  }

  return {
    label: os ? `${browser} on ${os}` : browser,
    isMobile,
  }
}

function formatDateTime(iso: string): string {
  if (!iso) return '—'
  const d = new Date(iso)
  if (Number.isNaN(d.getTime())) return iso
  return d.toLocaleString(undefined, {
    month: 'short',
    day: 'numeric',
    year: 'numeric',
    hour: '2-digit',
    minute: '2-digit',
  })
}

function formatRelativeTime(iso: string): string {
  if (!iso) return '—'
  const d = new Date(iso)
  if (Number.isNaN(d.getTime())) return iso
  const diffMs = Date.now() - d.getTime()
  const diffMins = Math.floor(diffMs / 60000)
  if (diffMins < 1) return 'Just now'
  if (diffMins < 60) return `${diffMins}m ago`
  const diffHours = Math.floor(diffMins / 60)
  if (diffHours < 24) return `${diffHours}h ago`
  const diffDays = Math.floor(diffHours / 24)
  if (diffDays < 7) return `${diffDays}d ago`
  return formatDateTime(iso)
}

export const AccountSettingsModal: React.FC<AccountSettingsModalProps> = ({
  open,
  initialTab,
  onClose,
  onUserUpdated,
  wanikaniStatus,
  knownKanji,
  onWaniKaniUpdated,
  onRestartOnboarding,
}) => {
  const { toast } = useToast()
  const [tab, setTab] = useState<SettingsTab>(initialTab ?? 'profile')
  const [loading, setLoading] = useState(false)
  const [loadError, setLoadError] = useState<string | null>(null)

  const [profile, setProfile] = useState<AccountProfileResponse | null>(null)
  const [sessions, setSessions] = useState<AccountSessionInfo[]>([])
  const [fallbackWkStatus, setFallbackWkStatus] = useState<WaniKaniStatus | null>(null)
  const [loadingWk, setLoadingWk] = useState(false)
  const [wkError, setWkError] = useState<string | null>(null)

  // Profile form state
  const [username, setUsername] = useState('')
  const [savingProfile, setSavingProfile] = useState(false)
  const [profileError, setProfileError] = useState<string | null>(null)

  // Password form state
  const [currentPassword, setCurrentPassword] = useState('')
  const [newPassword, setNewPassword] = useState('')
  const [confirmPassword, setConfirmPassword] = useState('')
  const [revokeOtherSessions, setRevokeOtherSessions] = useState(true)
  const [savingPassword, setSavingPassword] = useState(false)
  const [passwordError, setPasswordError] = useState<string | null>(null)

  // Sessions action state
  const [revokingId, setRevokingId] = useState<string | null>(null)
  const [revokingAll, setRevokingAll] = useState(false)

  // Email verification state
  const [resendingVerification, setResendingVerification] = useState(false)
  const [resendCooldown, setResendCooldown] = useState(0)

  // Onboarding restart state
  const [restartingOnboarding, setRestartingOnboarding] = useState(false)

  const wkStatus = wanikaniStatus ?? fallbackWkStatus

  const resolvedKnownKanji = useMemo(
    () => knownKanji ?? new Set(wkStatus?.known_kanji_list ?? []),
    [knownKanji, wkStatus]
  )

  const loadAccountData = useCallback(async () => {
    setLoading(true)
    setLoadError(null)
    try {
      const [profileResp, sessionsResp] = await Promise.all([
        api.getAccountProfile(),
        api.getAccountSessions(),
      ])
      setProfile(profileResp)
      setUsername(profileResp.user.username)
      setSessions(sessionsResp)
    } catch (err) {
      const msg = err instanceof Error ? err.message : 'Failed to load account settings.'
      setLoadError(msg)
      toast.error(msg)
    } finally {
      setLoading(false)
    }
  }, [toast])

  const loadFallbackWaniKani = useCallback(async () => {
    setLoadingWk(true)
    setWkError(null)
    try {
      const resp = await api.getWaniKaniStatus()
      setFallbackWkStatus(resp)
    } catch (err) {
      const msg = err instanceof Error ? err.message : 'Failed to load WaniKani status.'
      setWkError(msg)
    } finally {
      setLoadingWk(false)
    }
  }, [])

  const handleWkUpdated = useCallback(() => {
    onWaniKaniUpdated?.()
    if (!wanikaniStatus) {
      void loadFallbackWaniKani()
    }
  }, [onWaniKaniUpdated, wanikaniStatus, loadFallbackWaniKani])

  useEffect(() => {
    if (!open) return
    setTab(initialTab ?? 'profile')
  }, [open, initialTab])

  useEffect(() => {
    if (!open) return
    setProfileError(null)
    setPasswordError(null)
    setCurrentPassword('')
    setNewPassword('')
    setConfirmPassword('')
    setRevokeOtherSessions(true)
    void loadAccountData()
  }, [open, loadAccountData])

  useEffect(() => {
    if (!open || wanikaniStatus) return
    void loadFallbackWaniKani()
  }, [open, wanikaniStatus, loadFallbackWaniKani])

  useEffect(() => {
    if (!open) return
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        onClose()
      }
    }
    window.addEventListener('keydown', handleKeyDown)
    return () => window.removeEventListener('keydown', handleKeyDown)
  }, [open, onClose])

  useEffect(() => {
    if (resendCooldown <= 0) return
    const timer = window.setTimeout(() => {
      setResendCooldown((prev) => Math.max(0, prev - 1))
    }, 1000)
    return () => window.clearTimeout(timer)
  }, [resendCooldown])

  if (!open) return null

  const handleResendVerification = async () => {
    if (!profile || resendingVerification || resendCooldown > 0) return
    setResendingVerification(true)
    try {
      const res = await api.resendVerificationEmail()
      if (res.already_verified) {
        toast.success(res.message || 'Your email is already verified.')
        const updatedUser: AuthUser = { ...profile.user, email_verified: true }
        setProfile((prev) => (prev ? { ...prev, user: updatedUser } : prev))
        onUserUpdated(updatedUser)
      } else {
        toast.success(res.message || 'Verification email sent. Please check your inbox.')
        setResendCooldown(30)
      }
    } catch (err) {
      const msg = err instanceof Error ? err.message : 'Failed to send verification email.'
      toast.error(msg)
    } finally {
      setResendingVerification(false)
    }
  }

  const handleSaveProfile = async (e: React.FormEvent) => {
    e.preventDefault()
    const trimmed = username.trim()
    if (trimmed.length < 2 || trimmed.length > 32) {
      setProfileError('Username must be between 2 and 32 characters.')
      return
    }
    setSavingProfile(true)
    setProfileError(null)
    try {
      const updated = await api.updateAccountProfile(trimmed)
      setProfile(updated)
      setUsername(updated.user.username)
      onUserUpdated(updated.user)
      toast.success('Profile updated.')
    } catch (err) {
      const msg = err instanceof Error ? err.message : 'Failed to update profile.'
      setProfileError(msg)
      toast.error(msg)
    } finally {
      setSavingProfile(false)
    }
  }

  const handleRestartOnboarding = async () => {
    if (restartingOnboarding) return
    setRestartingOnboarding(true)
    try {
      await api.resetOnboarding()
      toast.info('Onboarding reset — launching setup wizard.')
      onClose()
      await onRestartOnboarding?.()
    } catch (err) {
      const msg = err instanceof Error ? err.message : 'Failed to reset onboarding wizard.'
      toast.error(msg)
    } finally {
      setRestartingOnboarding(false)
    }
  }

  const handleChangePassword = async (e: React.FormEvent) => {
    e.preventDefault()
    setPasswordError(null)

    if (profile?.has_password && !currentPassword) {
      setPasswordError('Please enter your current password.')
      return
    }
    if (newPassword.length < 8) {
      setPasswordError('New password must be at least 8 characters.')
      return
    }
    if (newPassword !== confirmPassword) {
      setPasswordError('New passwords do not match.')
      return
    }

    setSavingPassword(true)
    try {
      const resp = await api.changePassword({
        ...(profile?.has_password ? { current_password: currentPassword } : {}),
        new_password: newPassword,
        revoke_other_sessions: revokeOtherSessions,
      })
      setCurrentPassword('')
      setNewPassword('')
      setConfirmPassword('')
      toast.success(
        resp.message ||
          (profile?.has_password ? 'Password updated successfully.' : 'Password set successfully.')
      )
      // Refresh profile (has_password & session count) and active sessions list
      const [refreshedProfile, refreshedSessions] = await Promise.all([
        api.getAccountProfile(),
        api.getAccountSessions(),
      ])
      setProfile(refreshedProfile)
      setSessions(refreshedSessions)
    } catch (err) {
      const msg = err instanceof Error ? err.message : 'Failed to update password.'
      setPasswordError(msg)
      toast.error(msg)
    } finally {
      setSavingPassword(false)
    }
  }

  const handleRevokeSession = async (sessionId: string) => {
    setRevokingId(sessionId)
    try {
      await api.revokeSession(sessionId)
      setSessions((prev) => prev.filter((s) => s.id !== sessionId))
      setProfile((prev) =>
        prev ? { ...prev, active_sessions: Math.max(1, prev.active_sessions - 1) } : prev
      )
      toast.success('Session signed out.')
    } catch (err) {
      const msg = err instanceof Error ? err.message : 'Failed to revoke session.'
      toast.error(msg)
    } finally {
      setRevokingId(null)
    }
  }

  const handleRevokeAllOthers = async () => {
    setRevokingAll(true)
    try {
      const resp = await api.revokeOtherSessions()
      setSessions((prev) => prev.filter((s) => s.is_current))
      setProfile((prev) => (prev ? { ...prev, active_sessions: 1 } : prev))
      toast.success(
        resp.revoked === 1
          ? 'Signed out 1 other device.'
          : `Signed out ${resp.revoked} other devices.`
      )
    } catch (err) {
      const msg = err instanceof Error ? err.message : 'Failed to sign out other devices.'
      toast.error(msg)
    } finally {
      setRevokingAll(false)
    }
  }

  const otherSessionCount = sessions.filter((s) => !s.is_current).length
  const usernameChanged = profile ? username.trim() !== profile.user.username : false

  const tabs: {
    id: SettingsTab
    label: string
    icon: React.ReactNode
    badge?: number
    dotColor?: string
  }[] = [
    { id: 'profile', label: 'Profile', icon: <User className="h-3.5 w-3.5" /> },
    { id: 'password', label: 'Password', icon: <KeyRound className="h-3.5 w-3.5" /> },
    {
      id: 'sessions',
      label: 'Active Sessions',
      icon: <Monitor className="h-3.5 w-3.5" />,
      badge: sessions.length > 0 ? sessions.length : profile?.active_sessions,
    },
    {
      id: 'wanikani',
      label: 'WaniKani',
      icon: <Languages className="h-3.5 w-3.5" />,
      dotColor: wkStatus?.connected ? 'var(--wk-pink)' : undefined,
    },
  ]

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/50 p-4 backdrop-blur-xs"
      onClick={onClose}
      role="dialog"
      aria-modal="true"
      aria-labelledby="account-settings-title"
    >
      <div
        className={`max-h-[90vh] w-full ${tab === 'wanikani' ? 'max-w-3xl' : 'max-w-2xl'} overflow-y-auto rounded-2xl border p-6 sm:p-7 shadow-2xl transition-[max-width] duration-200`}
        style={{
          backgroundColor: 'var(--bg-surface)',
          borderColor: 'var(--border-subtle)',
        }}
        onClick={(e) => e.stopPropagation()}
      >
        {/* Modal Header */}
        <div className="flex items-start justify-between gap-4 pb-5 border-b" style={{ borderColor: 'var(--border-subtle)' }}>
          <div className="flex items-center gap-3">
            <div
              className="flex h-10 w-10 items-center justify-center rounded-xl"
              style={{
                backgroundColor: 'var(--accent-indigo-soft)',
                color: 'var(--accent-indigo)',
              }}
            >
              <Shield className="h-5 w-5" />
            </div>
            <div>
              <h2 id="account-settings-title" className="text-lg font-bold tracking-tight">
                Account &amp; Security
              </h2>
              <p className="text-xs" style={{ color: 'var(--text-muted)' }}>
                Manage your profile, password, active device sessions, and WaniKani integration.
              </p>
            </div>
          </div>
          <button
            type="button"
            onClick={onClose}
            className="flex h-8 w-8 items-center justify-center rounded-lg border transition-colors hover:opacity-80 cursor-pointer"
            style={{
              borderColor: 'var(--border-subtle)',
              backgroundColor: 'var(--bg-subtle)',
              color: 'var(--text-secondary)',
            }}
            aria-label="Close account settings"
          >
            <X className="h-4 w-4" />
          </button>
        </div>

        {/* Tab Strip */}
        <div
          className="mt-5 grid grid-cols-2 sm:grid-cols-4 gap-1 rounded-xl p-1"
          style={{ backgroundColor: 'var(--bg-subtle)' }}
          role="tablist"
        >
          {tabs.map((item) => {
            const active = tab === item.id
            return (
              <button
                key={item.id}
                type="button"
                role="tab"
                aria-selected={active}
                onClick={() => setTab(item.id)}
                className="flex items-center justify-center gap-1.5 rounded-lg px-2.5 py-2 text-xs font-semibold transition-colors cursor-pointer"
                style={{
                  backgroundColor: active ? 'var(--bg-surface)' : 'transparent',
                  color: active ? 'var(--text-primary)' : 'var(--text-secondary)',
                }}
              >
                {item.icon}
                <span>{item.label}</span>
                {item.badge !== undefined && item.badge > 0 && (
                  <span
                    className="font-mono-code rounded-full px-1.5 py-0.2 text-[10px]"
                    style={{
                      backgroundColor: active ? 'var(--accent-indigo-soft)' : 'var(--bg-canvas)',
                      color: active ? 'var(--accent-indigo)' : 'var(--text-muted)',
                    }}
                  >
                    {item.badge}
                  </span>
                )}
                {item.dotColor && (
                  <span
                    className="h-2 w-2 rounded-full shrink-0"
                    style={{ backgroundColor: item.dotColor }}
                    title="Connected"
                  />
                )}
              </button>
            )
          })}
        </div>

        {/* Body */}
        <div className="mt-6">
          {tab === 'wanikani' ? (
            wkStatus ? (
              <WaniKaniPage
                embedded
                status={wkStatus}
                knownKanji={resolvedKnownKanji}
                onUpdated={handleWkUpdated}
              />
            ) : loadingWk ? (
              <div className="py-12 text-center text-sm" style={{ color: 'var(--text-muted)' }}>
                Loading WaniKani settings...
              </div>
            ) : (
              <div
                className="rounded-xl border p-5 text-center"
                style={{
                  backgroundColor: 'var(--accent-vermilion-soft)',
                  borderColor: 'var(--accent-vermilion)',
                }}
              >
                <p className="text-sm font-medium" style={{ color: 'var(--accent-vermilion)' }}>
                  {wkError || 'Unable to load WaniKani status.'}
                </p>
                <button
                  type="button"
                  onClick={() => void loadFallbackWaniKani()}
                  className="mt-3 rounded-lg px-3.5 py-1.5 text-xs font-semibold text-white cursor-pointer"
                  style={{ backgroundColor: 'var(--accent-vermilion)' }}
                >
                  Retry
                </button>
              </div>
            )
          ) : loading ? (
            <div className="py-12 text-center text-sm" style={{ color: 'var(--text-muted)' }}>
              Loading account settings...
            </div>
          ) : loadError || !profile ? (
            <div
              className="rounded-xl border p-5 text-center"
              style={{
                backgroundColor: 'var(--accent-vermilion-soft)',
                borderColor: 'var(--accent-vermilion)',
              }}
            >
              <p className="text-sm font-medium" style={{ color: 'var(--accent-vermilion)' }}>
                {loadError || 'Unable to load account details.'}
              </p>
              <button
                type="button"
                onClick={() => void loadAccountData()}
                className="mt-3 rounded-lg px-3.5 py-1.5 text-xs font-semibold text-white cursor-pointer"
                style={{ backgroundColor: 'var(--accent-vermilion)' }}
              >
                Retry
              </button>
            </div>
          ) : (
            <>
              {/* PROFILE TAB */}
              {tab === 'profile' && (
                <div className="flex flex-col gap-5">
                  {/* Account Summary Card */}
                  <div
                    className="flex flex-col sm:flex-row sm:items-center justify-between gap-4 rounded-xl border p-4"
                    style={{
                      backgroundColor: 'var(--bg-canvas)',
                      borderColor: 'var(--border-subtle)',
                    }}
                  >
                    <div className="flex items-center gap-3.5 min-w-0">
                      {profile.user.avatar_url ? (
                        <img
                          src={profile.user.avatar_url}
                          alt={profile.user.username}
                          referrerPolicy="no-referrer"
                          className="h-12 w-12 shrink-0 rounded-full object-cover border"
                          style={{ borderColor: 'var(--border-subtle)' }}
                        />
                      ) : (
                        <div
                          className="flex h-12 w-12 shrink-0 items-center justify-center rounded-full text-base font-bold text-white"
                          style={{ backgroundColor: 'var(--accent-vermilion)' }}
                        >
                          {profile.user.username.slice(0, 1).toUpperCase()}
                        </div>
                      )}
                      <div className="min-w-0">
                        <div className="truncate text-sm font-bold">{profile.user.username}</div>
                        <div className="truncate text-xs" style={{ color: 'var(--text-secondary)' }}>
                          {profile.user.email}
                        </div>
                        <div className="mt-0.5 text-[11px]" style={{ color: 'var(--text-muted)' }}>
                          Member since {formatDateTime(profile.user.created_at)}
                        </div>
                      </div>
                    </div>

                    <div className="flex flex-wrap items-center gap-2">
                      {profile.user.email_verified ? (
                        <span
                          className="inline-flex items-center gap-1.5 rounded-full px-2.5 py-1 text-[11px] font-semibold"
                          style={{
                            backgroundColor: 'var(--accent-matcha-soft)',
                            color: 'var(--accent-matcha)',
                          }}
                        >
                          <CheckCircle2 className="h-3.5 w-3.5" />
                          Email Verified
                        </span>
                      ) : (
                        <>
                          <span
                            className="inline-flex items-center gap-1.5 rounded-full px-2.5 py-1 text-[11px] font-semibold"
                            style={{
                              backgroundColor: 'var(--accent-amber-soft)',
                              color: 'var(--accent-amber)',
                            }}
                          >
                            <Mail className="h-3.5 w-3.5" />
                            Email Unverified
                          </span>
                          <button
                            type="button"
                            onClick={() => void handleResendVerification()}
                            disabled={resendingVerification || resendCooldown > 0}
                            className="inline-flex items-center gap-1 rounded-full border px-2.5 py-1 text-[11px] font-semibold transition-opacity hover:opacity-85 disabled:opacity-50 cursor-pointer"
                            style={{
                              backgroundColor: 'var(--bg-surface)',
                              borderColor: 'var(--accent-amber)',
                              color: 'var(--accent-amber)',
                            }}
                          >
                            {resendingVerification
                              ? 'Sending...'
                              : resendCooldown > 0
                                ? `Resend in ${resendCooldown}s`
                                : 'Resend verification'}
                          </button>
                        </>
                      )}

                      <span
                        className="inline-flex items-center gap-1.5 rounded-full px-2.5 py-1 text-[11px] font-semibold"
                        style={{
                          backgroundColor: profile.google_linked
                            ? 'var(--accent-matcha-soft)'
                            : 'var(--bg-subtle)',
                          color: profile.google_linked
                            ? 'var(--accent-matcha)'
                            : 'var(--text-muted)',
                        }}
                      >
                        {profile.google_linked ? (
                          <CheckCircle2 className="h-3.5 w-3.5" />
                        ) : (
                          <Shield className="h-3.5 w-3.5" />
                        )}
                        {profile.google_linked ? 'Google Linked' : 'Google Not Linked'}
                      </span>

                      <span
                        className="font-mono-code inline-flex items-center gap-1.5 rounded-full px-2.5 py-1 text-[11px] font-semibold"
                        style={{
                          backgroundColor: 'var(--accent-indigo-soft)',
                          color: 'var(--accent-indigo)',
                        }}
                      >
                        <Laptop className="h-3.5 w-3.5" />
                        {profile.active_sessions}{' '}
                        {profile.active_sessions === 1 ? 'session' : 'sessions'}
                      </span>
                    </div>
                  </div>

                  {/* Username Edit Form */}
                  <form onSubmit={handleSaveProfile} className="flex flex-col gap-4">
                    <div>
                      <label
                        htmlFor="account-email"
                        className="mb-1.5 block text-xs font-semibold"
                        style={{ color: 'var(--text-secondary)' }}
                      >
                        Email Address
                      </label>
                      <input
                        id="account-email"
                        type="email"
                        value={profile.user.email}
                        disabled
                        className="w-full rounded-xl border px-3.5 py-2.5 text-sm opacity-75 cursor-not-allowed"
                        style={{
                          backgroundColor: 'var(--bg-subtle)',
                          borderColor: 'var(--border-subtle)',
                          color: 'var(--text-secondary)',
                        }}
                      />
                    </div>

                    <div>
                      <label
                        htmlFor="account-username"
                        className="mb-1.5 block text-xs font-semibold"
                        style={{ color: 'var(--text-secondary)' }}
                      >
                        Username
                      </label>
                      <input
                        id="account-username"
                        type="text"
                        required
                        minLength={2}
                        maxLength={32}
                        value={username}
                        onChange={(e) => {
                          setUsername(e.target.value)
                          setProfileError(null)
                        }}
                        placeholder="akira_jp"
                        className="w-full rounded-xl border px-3.5 py-2.5 text-sm outline-none"
                        style={{
                          backgroundColor: 'var(--bg-canvas)',
                          borderColor: 'var(--border-subtle)',
                          color: 'var(--text-primary)',
                        }}
                      />
                      <p className="mt-1 text-[11px]" style={{ color: 'var(--text-muted)' }}>
                        2–32 characters. Displayed in your navigation bar and study workspace.
                      </p>
                    </div>

                    {profileError && (
                      <div
                        className="rounded-xl border px-3.5 py-2.5 text-xs font-medium"
                        style={{
                          backgroundColor: 'var(--accent-vermilion-soft)',
                          borderColor: 'var(--accent-vermilion)',
                          color: 'var(--accent-vermilion)',
                        }}
                      >
                        {profileError}
                      </div>
                    )}

                    <div className="flex justify-end">
                      <button
                        type="submit"
                        disabled={savingProfile || !usernameChanged}
                        className="rounded-xl px-5 py-2.5 text-xs font-semibold text-white transition-opacity hover:opacity-90 disabled:opacity-50 cursor-pointer"
                        style={{ backgroundColor: 'var(--accent-vermilion)' }}
                      >
                        {savingProfile ? 'Saving...' : 'Save Profile'}
                      </button>
                    </div>
                  </form>

                  {/* Study Setup Card */}
                  <div
                    className="flex flex-col sm:flex-row sm:items-center justify-between gap-3 rounded-xl border p-4"
                    style={{
                      backgroundColor: 'var(--bg-canvas)',
                      borderColor: 'var(--border-subtle)',
                    }}
                  >
                    <div className="flex items-start gap-3">
                      <div
                        className="flex h-9 w-9 shrink-0 items-center justify-center rounded-lg"
                        style={{
                          backgroundColor: 'var(--accent-indigo-soft)',
                          color: 'var(--accent-indigo)',
                        }}
                      >
                        <Compass className="h-4 w-4" />
                      </div>
                      <div>
                        <div className="text-xs font-bold">Study Setup</div>
                        <p className="mt-0.5 text-[11px] leading-relaxed" style={{ color: 'var(--text-secondary)' }}>
                          Revisit your study path, daily goal, JLPT placement check, and WaniKani link.
                        </p>
                      </div>
                    </div>
                    <button
                      type="button"
                      disabled={restartingOnboarding}
                      onClick={() => void handleRestartOnboarding()}
                      className="shrink-0 rounded-xl border px-3.5 py-2 text-xs font-semibold transition-opacity hover:opacity-85 disabled:opacity-50 cursor-pointer"
                      style={{
                        borderColor: 'var(--border-subtle)',
                        backgroundColor: 'var(--bg-surface)',
                        color: 'var(--text-primary)',
                      }}
                    >
                      {restartingOnboarding ? 'Launching...' : 'Re-run Onboarding Wizard'}
                    </button>
                  </div>
                </div>
              )}

              {/* PASSWORD TAB */}
              {tab === 'password' && (
                <form onSubmit={handleChangePassword} className="flex flex-col gap-4">
                  {!profile.has_password && (
                    <div
                      className="flex items-start gap-2.5 rounded-xl border px-3.5 py-3 text-xs leading-relaxed"
                      style={{
                        backgroundColor: 'var(--accent-indigo-soft)',
                        borderColor: 'var(--accent-indigo)',
                        color: 'var(--text-primary)',
                      }}
                    >
                      <ShieldCheck
                        className="mt-0.5 h-4 w-4 shrink-0"
                        style={{ color: 'var(--accent-indigo)' }}
                      />
                      <span>
                        Your account currently signs in with Google OAuth only. Set a password below
                        if you would also like to sign in with email and password.
                      </span>
                    </div>
                  )}

                  {passwordError && (
                    <div
                      className="rounded-xl border px-3.5 py-2.5 text-xs font-medium"
                      style={{
                        backgroundColor: 'var(--accent-vermilion-soft)',
                        borderColor: 'var(--accent-vermilion)',
                        color: 'var(--accent-vermilion)',
                      }}
                    >
                      {passwordError}
                    </div>
                  )}

                  {profile.has_password && (
                    <div>
                      <label
                        htmlFor="current-password"
                        className="mb-1.5 block text-xs font-semibold"
                        style={{ color: 'var(--text-secondary)' }}
                      >
                        Current Password
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
                          id="current-password"
                          type="password"
                          required
                          autoComplete="current-password"
                          value={currentPassword}
                          onChange={(e) => setCurrentPassword(e.target.value)}
                          placeholder="Enter current password"
                          className="w-full bg-transparent text-sm outline-none"
                          style={{ color: 'var(--text-primary)' }}
                        />
                      </div>
                    </div>
                  )}

                  <div>
                    <label
                      htmlFor="new-password"
                      className="mb-1.5 block text-xs font-semibold"
                      style={{ color: 'var(--text-secondary)' }}
                    >
                      New Password
                    </label>
                    <div
                      className="flex items-center gap-2.5 rounded-xl border px-3.5 py-2.5"
                      style={{
                        backgroundColor: 'var(--bg-canvas)',
                        borderColor: 'var(--border-subtle)',
                      }}
                    >
                      <KeyRound className="h-4 w-4 shrink-0" style={{ color: 'var(--text-muted)' }} />
                      <input
                        id="new-password"
                        type="password"
                        required
                        minLength={8}
                        autoComplete="new-password"
                        value={newPassword}
                        onChange={(e) => setNewPassword(e.target.value)}
                        placeholder="At least 8 characters"
                        className="w-full bg-transparent text-sm outline-none"
                        style={{ color: 'var(--text-primary)' }}
                      />
                    </div>
                  </div>

                  <div>
                    <label
                      htmlFor="confirm-new-password"
                      className="mb-1.5 block text-xs font-semibold"
                      style={{ color: 'var(--text-secondary)' }}
                    >
                      Confirm New Password
                    </label>
                    <div
                      className="flex items-center gap-2.5 rounded-xl border px-3.5 py-2.5"
                      style={{
                        backgroundColor: 'var(--bg-canvas)',
                        borderColor: 'var(--border-subtle)',
                      }}
                    >
                      <KeyRound className="h-4 w-4 shrink-0" style={{ color: 'var(--text-muted)' }} />
                      <input
                        id="confirm-new-password"
                        type="password"
                        required
                        minLength={8}
                        autoComplete="new-password"
                        value={confirmPassword}
                        onChange={(e) => setConfirmPassword(e.target.value)}
                        placeholder="Re-enter new password"
                        className="w-full bg-transparent text-sm outline-none"
                        style={{ color: 'var(--text-primary)' }}
                      />
                    </div>
                  </div>

                  <label
                    className="flex items-center gap-2.5 rounded-xl border px-3.5 py-2.5 text-xs cursor-pointer select-none"
                    style={{
                      backgroundColor: 'var(--bg-canvas)',
                      borderColor: 'var(--border-subtle)',
                      color: 'var(--text-secondary)',
                    }}
                  >
                    <input
                      type="checkbox"
                      checked={revokeOtherSessions}
                      onChange={(e) => setRevokeOtherSessions(e.target.checked)}
                      className="h-4 w-4 rounded accent-[var(--accent-vermilion)] cursor-pointer"
                    />
                    <span className="font-medium">Sign out all other devices</span>
                  </label>

                  <div className="flex justify-end pt-1">
                    <button
                      type="submit"
                      disabled={savingPassword}
                      className="rounded-xl px-5 py-2.5 text-xs font-semibold text-white transition-opacity hover:opacity-90 disabled:opacity-50 cursor-pointer"
                      style={{ backgroundColor: 'var(--accent-vermilion)' }}
                    >
                      {savingPassword
                        ? 'Updating...'
                        : profile.has_password
                          ? 'Update Password'
                          : 'Set Password'}
                    </button>
                  </div>
                </form>
              )}

              {/* ACTIVE SESSIONS TAB */}
              {tab === 'sessions' && (
                <div className="flex flex-col gap-4">
                  <div className="flex flex-wrap items-center justify-between gap-3">
                    <div>
                      <h3 className="text-sm font-bold">Signed-in Devices</h3>
                      <p className="text-xs" style={{ color: 'var(--text-muted)' }}>
                        Revoke any session you do not recognize to immediately terminate access.
                      </p>
                    </div>
                    <button
                      type="button"
                      disabled={revokingAll || otherSessionCount === 0}
                      onClick={() => void handleRevokeAllOthers()}
                      className="inline-flex items-center gap-1.5 rounded-xl border px-3.5 py-2 text-xs font-semibold transition-opacity hover:opacity-85 disabled:opacity-45 cursor-pointer"
                      style={{
                        backgroundColor: 'var(--accent-vermilion-soft)',
                        borderColor: 'var(--accent-vermilion)',
                        color: 'var(--accent-vermilion)',
                      }}
                    >
                      <LogOut className="h-3.5 w-3.5" />
                      <span>
                        {revokingAll ? 'Signing out...' : 'Sign Out All Other Devices'}
                      </span>
                    </button>
                  </div>

                  {sessions.length === 0 ? (
                    <div
                      className="rounded-xl border py-8 text-center text-xs"
                      style={{
                        backgroundColor: 'var(--bg-canvas)',
                        borderColor: 'var(--border-subtle)',
                        color: 'var(--text-muted)',
                      }}
                    >
                      No active sessions found.
                    </div>
                  ) : (
                    <div className="flex flex-col gap-2.5">
                      {sessions.map((s) => {
                        const ua = parseUserAgent(s.user_agent)
                        const isRevoking = revokingId === s.id
                        return (
                          <div
                            key={s.id}
                            className="flex flex-col sm:flex-row sm:items-center justify-between gap-3 rounded-xl border px-4 py-3"
                            style={{
                              backgroundColor: 'var(--bg-canvas)',
                              borderColor: s.is_current
                                ? 'var(--accent-matcha)'
                                : 'var(--border-subtle)',
                            }}
                          >
                            <div className="flex items-start gap-3 min-w-0">
                              <div
                                className="mt-0.5 flex h-9 w-9 shrink-0 items-center justify-center rounded-lg"
                                style={{
                                  backgroundColor: s.is_current
                                    ? 'var(--accent-matcha-soft)'
                                    : 'var(--bg-subtle)',
                                  color: s.is_current
                                    ? 'var(--accent-matcha)'
                                    : 'var(--text-secondary)',
                                }}
                              >
                                {ua.isMobile ? (
                                  <Smartphone className="h-4 w-4" />
                                ) : (
                                  <Laptop className="h-4 w-4" />
                                )}
                              </div>
                              <div className="min-w-0">
                                <div className="flex flex-wrap items-center gap-2">
                                  <span
                                    className="text-sm font-semibold truncate"
                                    title={s.user_agent || undefined}
                                  >
                                    {ua.label}
                                  </span>
                                  {s.is_current && (
                                    <span
                                      className="rounded-full px-2 py-0.5 text-[10px] font-bold"
                                      style={{
                                        backgroundColor: 'var(--accent-matcha-soft)',
                                        color: 'var(--accent-matcha)',
                                      }}
                                    >
                                      Current device
                                    </span>
                                  )}
                                </div>
                                <div
                                  className="mt-0.5 flex flex-wrap items-center gap-x-3 gap-y-0.5 text-[11px]"
                                  style={{ color: 'var(--text-muted)' }}
                                >
                                  <span className="font-mono-code">
                                    {s.ip_address || 'Unknown IP'}
                                  </span>
                                  <span>Last seen {formatRelativeTime(s.last_seen_at)}</span>
                                  <span>Signed in {formatDateTime(s.created_at)}</span>
                                </div>
                              </div>
                            </div>

                            {!s.is_current && (
                              <button
                                type="button"
                                disabled={isRevoking || revokingAll}
                                onClick={() => void handleRevokeSession(s.id)}
                                className="inline-flex shrink-0 items-center justify-center gap-1.5 rounded-lg border px-3 py-1.5 text-xs font-semibold transition-colors hover:opacity-85 disabled:opacity-50 cursor-pointer"
                                style={{
                                  borderColor: 'var(--border-subtle)',
                                  backgroundColor: 'var(--bg-surface)',
                                  color: 'var(--accent-vermilion)',
                                }}
                              >
                                <Trash2 className="h-3.5 w-3.5" />
                                <span>{isRevoking ? 'Revoking...' : 'Revoke'}</span>
                              </button>
                            )}
                          </div>
                        )
                      })}
                    </div>
                  )}
                </div>
              )}
            </>
          )}
        </div>
      </div>
    </div>
  )
}
