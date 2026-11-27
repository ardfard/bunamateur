import React, { useEffect, useState } from 'react'
import { Mail, X } from 'lucide-react'
import { api } from '../api'
import type { AuthUser } from '../types'
import { useToast } from './ToastContext'

interface EmailVerificationBannerProps {
  user: AuthUser
  onVerifiedRefresh?: () => void
}

const RESEND_COOLDOWN_SECONDS = 30

export const EmailVerificationBanner: React.FC<EmailVerificationBannerProps> = ({
  user,
  onVerifiedRefresh,
}) => {
  const { toast } = useToast()
  const [dismissed, setDismissed] = useState(false)
  const [sending, setSending] = useState(false)
  const [cooldown, setCooldown] = useState(0)

  useEffect(() => {
    if (cooldown <= 0) return
    const timer = window.setTimeout(() => {
      setCooldown((prev) => Math.max(0, prev - 1))
    }, 1000)
    return () => window.clearTimeout(timer)
  }, [cooldown])

  if (user.email_verified || dismissed) {
    return null
  }

  const handleResend = async () => {
    if (sending || cooldown > 0) return
    setSending(true)
    try {
      const res = await api.resendVerificationEmail()
      if (res.already_verified) {
        toast.success(res.message || 'Your email is already verified.')
        onVerifiedRefresh?.()
      } else {
        toast.success(res.message || 'Verification email sent. Please check your inbox.')
        setCooldown(RESEND_COOLDOWN_SECONDS)
      }
    } catch (err) {
      const msg = err instanceof Error ? err.message : 'Failed to send verification email.'
      toast.error(msg)
    } finally {
      setSending(false)
    }
  }

  return (
    <div
      role="status"
      className="w-full border-b transition-colors"
      style={{
        backgroundColor: 'var(--accent-amber-soft)',
        borderColor: 'var(--border-subtle)',
        color: 'var(--text-primary)',
      }}
    >
      <div className="mx-auto flex max-w-[1360px] flex-wrap items-center justify-between gap-2.5 px-4 py-2 text-xs sm:px-8">
        <div className="flex min-w-0 items-center gap-2">
          <Mail className="h-4 w-4 shrink-0" style={{ color: 'var(--accent-amber)' }} />
          <p className="leading-snug font-medium">
            Please verify your email ({user.email}) — check your inbox for the confirmation link.
          </p>
        </div>

        <div className="flex shrink-0 items-center gap-2">
          <button
            type="button"
            onClick={() => void handleResend()}
            disabled={sending || cooldown > 0}
            className="rounded-lg border px-2.5 py-1 text-xs font-semibold transition-opacity hover:opacity-85 disabled:opacity-55 cursor-pointer"
            style={{
              backgroundColor: 'var(--bg-surface)',
              borderColor: 'var(--accent-amber)',
              color: 'var(--accent-amber)',
            }}
          >
            {sending ? 'Sending...' : cooldown > 0 ? `Resend in ${cooldown}s` : 'Resend email'}
          </button>

          <button
            type="button"
            onClick={() => setDismissed(true)}
            className="flex h-6 w-6 items-center justify-center rounded-md transition-opacity hover:opacity-75 cursor-pointer"
            style={{ color: 'var(--text-secondary)' }}
            aria-label="Dismiss email verification reminder"
            title="Dismiss for this session"
          >
            <X className="h-3.5 w-3.5" />
          </button>
        </div>
      </div>
    </div>
  )
}
