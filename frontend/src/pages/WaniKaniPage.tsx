import React, { useEffect, useState } from 'react'
import { CheckCircle2, Clock, KeyRound, RefreshCw, Sparkles, Unlink } from 'lucide-react'
import { api } from '../api'
import type { WaniKaniStatus } from '../types'
import { RubyText } from '../utils/furigana'

interface WaniKaniPageProps {
  status: WaniKaniStatus
  knownKanji: Set<string>
  onUpdated: () => void
}

export const WaniKaniPage: React.FC<WaniKaniPageProps> = ({
  status,
  knownKanji,
  onUpdated,
}) => {
  const [token, setToken] = useState('')
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [successMsg, setSuccessMsg] = useState<string | null>(null)

  // Poll sync progress while a background WaniKani job is running
  useEffect(() => {
    if (status.sync_status !== 'running') return
    const timer = window.setInterval(async () => {
      try {
        const prog = await api.getWaniKaniSyncStatus()
        onUpdated()
        if (prog.sync_status === 'completed') {
          setSuccessMsg(prog.sync_message || 'WaniKani sync complete!')
        } else if (prog.sync_status === 'failed') {
          setError(prog.sync_error || 'Background WaniKani sync failed')
        }
      } catch {
        // Ignore transient poll hiccup
      }
    }, 1500)
    return () => window.clearInterval(timer)
  }, [status.sync_status, onUpdated])

  const handleConnect = async (e: React.FormEvent) => {
    e.preventDefault()
    if (!token.trim()) return
    setBusy(true)
    setError(null)
    setSuccessMsg(null)
    try {
      const res = await api.connectWaniKani(token.trim())
      setToken('')
      setSuccessMsg(
        `Connected as ${res.username} (Level ${res.level}) — background sync started!`
      )
      onUpdated()
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to connect WaniKani')
    } finally {
      setBusy(false)
    }
  }

  const handleDemo = async () => {
    setBusy(true)
    setError(null)
    setSuccessMsg(null)
    try {
      const res = await api.demoWaniKani()
      setSuccessMsg(
        `Demo WaniKani Level ${res.level} activated with ${res.kanji_known} mastered kanji!`
      )
      onUpdated()
    } finally {
      setBusy(false)
    }
  }

  const handleResync = async (full = false) => {
    setBusy(true)
    setError(null)
    try {
      await api.syncWaniKani(full)
      setSuccessMsg(full ? 'Full WaniKani resync started...' : 'Incremental WaniKani sync started...')
      onUpdated()
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Sync failed')
    } finally {
      setBusy(false)
    }
  }

  const handleDisconnect = async () => {
    setBusy(true)
    try {
      await api.disconnectWaniKani()
      setSuccessMsg('WaniKani disconnected.')
      onUpdated()
    } finally {
      setBusy(false)
    }
  }

  const toggleSetting = async (field: 'smart_furigana' | 'highlight_known_kanji' | 'auto_sync_enabled') => {
    const nextSmart = field === 'smart_furigana' ? !status.smart_furigana : status.smart_furigana
    const nextHigh =
      field === 'highlight_known_kanji'
        ? !status.highlight_known_kanji
        : status.highlight_known_kanji
    const nextAuto =
      field === 'auto_sync_enabled' ? !status.auto_sync_enabled : status.auto_sync_enabled
    await api.updateWaniKaniSettings(nextSmart, nextHigh, nextAuto, status.auto_sync_interval_mins)
    onUpdated()
  }

  const changeAutoSyncInterval = async (mins: number) => {
    await api.updateWaniKaniSettings(
      status.smart_furigana,
      status.highlight_known_kanji,
      status.auto_sync_enabled,
      mins
    )
    onUpdated()
  }

  return (
    <div className="mx-auto flex max-w-[1040px] flex-col gap-6 px-4 py-8 sm:px-8">
      <div>
        <h1 className="text-2xl font-bold tracking-tight">WaniKani API v2 Integration</h1>
        <p className="mt-1 text-sm" style={{ color: 'var(--text-secondary)' }}>
          Sync your WaniKani kanji & vocabulary directly through the Rust backend. BunAmateur uses your Guru+ kanji to power Smart Furigana and Kanji Readiness scores.
        </p>
      </div>

      {error && (
        <div
          className="rounded-xl border px-4 py-3 text-sm font-medium"
          style={{
            backgroundColor: 'var(--accent-vermilion-soft)',
            borderColor: 'var(--accent-vermilion)',
            color: 'var(--accent-vermilion)',
          }}
        >
          {error}
        </div>
      )}

      {successMsg && (
        <div
          className="flex items-center gap-2 rounded-xl border px-4 py-3 text-sm font-medium"
          style={{
            backgroundColor: 'var(--accent-matcha-soft)',
            borderColor: 'var(--accent-matcha)',
            color: 'var(--accent-matcha)',
          }}
        >
          <CheckCircle2 className="h-4 w-4 shrink-0" />
          <span>{successMsg}</span>
        </div>
      )}

      {/* Connection Status Card */}
      <div
        className="flex flex-col gap-5 rounded-2xl border p-6"
        style={{
          backgroundColor: 'var(--bg-surface)',
          borderColor: status.connected ? 'var(--wk-pink)' : 'var(--border-subtle)',
        }}
      >
        <div className="flex flex-wrap items-center justify-between gap-4">
          <div>
            <div className="flex items-center gap-2.5">
              <h2 className="text-lg font-bold">
                {status.connected ? status.username || 'WaniKani Connected' : 'Connect Your WaniKani Account'}
              </h2>
              {status.connected && (
                <span
                  className="font-mono-code rounded-full px-2.5 py-0.5 text-xs font-bold text-white"
                  style={{ backgroundColor: 'var(--wk-pink)' }}
                >
                  Level {status.level}
                </span>
              )}
              {status.is_demo && (
                <span
                  className="font-mono-code rounded-full px-2.5 py-0.5 text-[11px] font-semibold"
                  style={{ backgroundColor: 'var(--accent-amber-soft)', color: 'var(--accent-amber)' }}
                >
                  Demo Mode
                </span>
              )}
            </div>
            {status.last_synced_at && (
              <p className="font-mono-code mt-1 text-xs" style={{ color: 'var(--text-muted)' }}>
                Last synced: {new Date(status.last_synced_at).toLocaleString()}
              </p>
            )}
          </div>

          {status.connected && (
            <div className="flex flex-wrap items-center gap-2">
              <button
                onClick={() => handleResync(false)}
                disabled={busy || status.sync_status === 'running'}
                className="flex items-center gap-1.5 rounded-xl px-4 py-2 text-xs font-semibold text-white cursor-pointer disabled:opacity-60"
                style={{ backgroundColor: 'var(--wk-pink)' }}
              >
                <RefreshCw
                  className={`h-3.5 w-3.5 ${busy || status.sync_status === 'running' ? 'animate-spin' : ''}`}
                />
                {status.sync_status === 'running' ? 'Syncing...' : 'Sync Now'}
              </button>
              {!status.is_demo && (
                <button
                  onClick={() => handleResync(true)}
                  disabled={busy || status.sync_status === 'running'}
                  className="rounded-xl border px-3 py-2 text-xs font-semibold cursor-pointer disabled:opacity-60"
                  style={{
                    borderColor: 'var(--border-subtle)',
                    color: 'var(--text-secondary)',
                  }}
                >
                  Full Resync
                </button>
              )}
              <button
                onClick={handleDisconnect}
                disabled={busy}
                className="flex items-center gap-1.5 rounded-xl border px-3.5 py-2 text-xs font-semibold cursor-pointer"
                style={{
                  borderColor: 'var(--border-subtle)',
                  color: 'var(--text-secondary)',
                }}
              >
                <Unlink className="h-3.5 w-3.5" />
                Disconnect
              </button>
            </div>
          )}
        </div>

        {/* Live Background Sync Progress / Error Banner */}
        {status.sync_status === 'running' && (
          <div
            className="flex items-center justify-between gap-3 rounded-xl border px-4 py-3 text-xs"
            style={{
              backgroundColor: 'var(--wk-pink-soft)',
              borderColor: 'var(--wk-pink)',
              color: 'var(--wk-pink)',
            }}
          >
            <div className="flex items-center gap-2 font-semibold">
              <RefreshCw className="h-3.5 w-3.5 animate-spin shrink-0" />
              <span>{status.sync_message || 'Syncing WaniKani progress in background...'}</span>
            </div>
            <span className="font-mono-code shrink-0">
              {status.sync_progress_pages} pages • {status.sync_progress_items} items
            </span>
          </div>
        )}

        {status.sync_status === 'failed' && status.sync_error && (
          <div
            className="rounded-xl border px-4 py-3 text-xs font-medium"
            style={{
              backgroundColor: 'var(--accent-vermilion-soft)',
              borderColor: 'var(--accent-vermilion)',
              color: 'var(--accent-vermilion)',
            }}
          >
            Last background sync failed: {status.sync_error}
          </div>
        )}

        {/* Connect Form */}
        <form onSubmit={handleConnect} className="flex flex-col gap-3 sm:flex-row">
          <div
            className="flex flex-1 items-center gap-2.5 rounded-xl border px-3.5 py-2.5"
            style={{
              backgroundColor: 'var(--bg-canvas)',
              borderColor: 'var(--border-subtle)',
            }}
          >
            <KeyRound className="h-4 w-4 shrink-0" style={{ color: 'var(--text-muted)' }} />
            <input
              type="password"
              value={token}
              onChange={(e) => setToken(e.target.value)}
              placeholder="Paste WaniKani Personal Access Token (v2, read-only)..."
              className="w-full bg-transparent text-sm outline-none"
              style={{ color: 'var(--text-primary)' }}
            />
          </div>
          <button
            type="submit"
            disabled={busy || !token.trim()}
            className="rounded-xl px-5 py-2.5 text-sm font-semibold text-white disabled:opacity-50 cursor-pointer"
            style={{ backgroundColor: 'var(--wk-pink)' }}
          >
            {busy ? 'Syncing...' : 'Connect & Sync'}
          </button>
          <button
            type="button"
            onClick={handleDemo}
            disabled={busy}
            className="flex items-center justify-center gap-1.5 rounded-xl border px-4 py-2.5 text-sm font-semibold cursor-pointer"
            style={{
              borderColor: 'var(--wk-pink)',
              backgroundColor: 'var(--wk-pink-soft)',
              color: 'var(--wk-pink)',
            }}
          >
            <Sparkles className="h-4 w-4" />
            Try Demo Lv.14
          </button>
        </form>

        <p className="text-xs" style={{ color: 'var(--text-muted)' }}>
          Generate a read-only API v2 token at <strong>wanikani.com/settings/personal_access_tokens</strong>. Your token is encrypted at rest (AES-256-GCM) in PostgreSQL and never sent anywhere except <code>api.wanikani.com/v2</code>.
        </p>
      </div>

      {/* Live Smart Furigana Controls + Preview */}
      <div className="grid grid-cols-1 gap-6 md:grid-cols-2">
        <div
          className="flex flex-col gap-4 rounded-2xl border p-6"
          style={{
            backgroundColor: 'var(--bg-surface)',
            borderColor: 'var(--border-subtle)',
          }}
        >
          <h3 className="text-base font-bold">Smart Furigana Behavior</h3>

          <label className="flex items-center justify-between gap-4 cursor-pointer">
            <div>
              <div className="text-sm font-semibold">Hide Furigana on Mastered Kanji</div>
              <div className="text-xs" style={{ color: 'var(--text-secondary)' }}>
                Automatically hide readings above kanji passed (Guru+) in WaniKani. Hover any kanji to peek.
              </div>
            </div>
            <input
              type="checkbox"
              checked={status.smart_furigana}
              onChange={() => toggleSetting('smart_furigana')}
              className="h-5 w-5 accent-pink-600 cursor-pointer"
            />
          </label>

          <label className="flex items-center justify-between gap-4 cursor-pointer">
            <div>
              <div className="text-sm font-semibold">Subtle WaniKani Kanji Underline</div>
              <div className="text-xs" style={{ color: 'var(--text-secondary)' }}>
                Show a delicate pink dotted underline under kanji synced from WaniKani.
              </div>
            </div>
            <input
              type="checkbox"
              checked={status.highlight_known_kanji}
              onChange={() => toggleSetting('highlight_known_kanji')}
              className="h-5 w-5 accent-pink-600 cursor-pointer"
            />
          </label>

          <div className="border-t pt-3 flex flex-col gap-3" style={{ borderColor: 'var(--border-subtle)' }}>
            <label className="flex items-center justify-between gap-4 cursor-pointer">
              <div>
                <div className="flex items-center gap-1.5 text-sm font-semibold">
                  <Clock className="h-3.5 w-3.5" style={{ color: 'var(--wk-pink)' }} />
                  Scheduled Incremental Auto-Sync
                </div>
                <div className="text-xs" style={{ color: 'var(--text-secondary)' }}>
                  Automatically fetches only updated assignments in the background using ETag &amp; updated_after.
                </div>
              </div>
              <input
                type="checkbox"
                checked={status.auto_sync_enabled}
                onChange={() => toggleSetting('auto_sync_enabled')}
                className="h-5 w-5 accent-pink-600 cursor-pointer"
              />
            </label>

            {status.auto_sync_enabled && (
              <div className="flex items-center justify-between gap-3 text-xs">
                <span style={{ color: 'var(--text-secondary)' }}>Auto-sync cadence</span>
                <select
                  value={status.auto_sync_interval_mins}
                  onChange={(e) => changeAutoSyncInterval(Number(e.target.value))}
                  className="font-mono-code rounded-lg border px-2.5 py-1.5 text-xs outline-none"
                  style={{
                    backgroundColor: 'var(--bg-canvas)',
                    borderColor: 'var(--border-subtle)',
                    color: 'var(--text-primary)',
                  }}
                >
                  <option value={30}>Every 30 min</option>
                  <option value={60}>Every 1 hour</option>
                  <option value={180}>Every 3 hours</option>
                  <option value={360}>Every 6 hours</option>
                  <option value={1440}>Daily</option>
                </select>
              </div>
            )}
          </div>
        </div>

        <div
          className="flex flex-col gap-3 rounded-2xl border p-6"
          style={{
            backgroundColor: 'var(--bg-surface)',
            borderColor: 'var(--border-subtle)',
          }}
        >
          <h3 className="text-base font-bold">Live Smart Furigana Preview</h3>
          <p className="text-xs" style={{ color: 'var(--text-secondary)' }}>
            Notice how basic kanji (like 毎日, 日本語, 勉強) drop their furigana once synced, while harder kanji (like 曖昧 or 納豆) keep their readings!
          </p>
          <div
            className="rounded-xl border p-4 font-jp text-xl leading-[2.3]"
            style={{
              backgroundColor: 'var(--bg-canvas)',
              borderColor: 'var(--border-subtle)',
            }}
          >
            <RubyText
              text="[毎日|まいにち][日本語|にほんご]を[勉強|べんきょう]しているおかげで、[納豆|なっとう]や[富士山|ふじさん]の[新聞|しんぶん]も[読|よ]めます。"
              furiganaMode={status.smart_furigana ? 'smart' : 'all'}
              knownKanji={knownKanji}
              highlightKnown={status.highlight_known_kanji}
            />
          </div>
        </div>
      </div>

      {/* Synced Kanji Showcase */}
      {status.connected && status.known_kanji_list.length > 0 && (
        <div
          className="flex flex-col gap-3 rounded-2xl border p-6"
          style={{
            backgroundColor: 'var(--bg-surface)',
            borderColor: 'var(--border-subtle)',
          }}
        >
          <div className="flex items-center justify-between">
            <h3 className="text-base font-bold">
              Synced WaniKani Kanji ({status.kanji_known} mastered)
            </h3>
            <span className="font-mono-code text-xs" style={{ color: 'var(--wk-pink)' }}>
              Showing first 80
            </span>
          </div>
          <div className="flex flex-wrap gap-1.5">
            {status.known_kanji_list.slice(0, 80).map((k) => (
              <span
                key={k}
                className="font-jp flex h-9 w-9 items-center justify-center rounded-lg text-base font-bold"
                style={{
                  backgroundColor: 'var(--wk-pink-soft)',
                  color: 'var(--wk-pink)',
                }}
              >
                {k}
              </span>
            ))}
          </div>
        </div>
      )}
    </div>
  )
}
