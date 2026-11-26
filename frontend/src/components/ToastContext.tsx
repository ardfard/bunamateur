import React, { createContext, useCallback, useContext, useEffect, useMemo, useRef, useState } from 'react'
import { CheckCircle2, Info, X, XCircle } from 'lucide-react'

type ToastVariant = 'success' | 'error' | 'info'

interface ToastItem {
  id: number
  variant: ToastVariant
  message: string
}

interface ToastApi {
  success: (message: string) => void
  error: (message: string) => void
  info: (message: string) => void
}

interface ToastContextValue {
  toast: ToastApi
  dismiss: (id: number) => void
}

const ToastContext = createContext<ToastContextValue | null>(null)

export const ToastProvider: React.FC<{ children: React.ReactNode }> = ({ children }) => {
  const [items, setItems] = useState<ToastItem[]>([])
  const nextIdRef = useRef(1)
  const timersRef = useRef<Map<number, number>>(new Map())

  const dismiss = useCallback((id: number) => {
    const timer = timersRef.current.get(id)
    if (timer !== undefined) {
      window.clearTimeout(timer)
      timersRef.current.delete(id)
    }
    setItems((prev) => prev.filter((item) => item.id !== id))
  }, [])

  const push = useCallback(
    (variant: ToastVariant, message: string) => {
      const id = nextIdRef.current++
      setItems((prev) => [...prev.slice(-3), { id, variant, message }])
      const duration = variant === 'error' ? 5000 : 3400
      const timer = window.setTimeout(() => {
        timersRef.current.delete(id)
        setItems((prev) => prev.filter((item) => item.id !== id))
      }, duration)
      timersRef.current.set(id, timer)
    },
    []
  )

  useEffect(() => {
    return () => {
      for (const timer of timersRef.current.values()) {
        window.clearTimeout(timer)
      }
      timersRef.current.clear()
    }
  }, [])

  const value = useMemo<ToastContextValue>(
    () => ({
      toast: {
        success: (msg) => push('success', msg),
        error: (msg) => push('error', msg),
        info: (msg) => push('info', msg),
      },
      dismiss,
    }),
    [push, dismiss]
  )

  return (
    <ToastContext.Provider value={value}>
      {children}
      {items.length > 0 && (
        <div
          className="pointer-events-none fixed right-4 bottom-4 z-50 flex w-full max-w-sm flex-col gap-2 sm:right-6 sm:bottom-6"
          aria-live="polite"
          role="region"
          aria-label="Notifications"
        >
          {items.map((item) => {
            const palette =
              item.variant === 'success'
                ? {
                    bg: 'var(--bg-surface)',
                    border: 'var(--accent-matcha)',
                    icon: 'var(--accent-matcha)',
                  }
                : item.variant === 'error'
                  ? {
                      bg: 'var(--bg-surface)',
                      border: 'var(--accent-vermilion)',
                      icon: 'var(--accent-vermilion)',
                    }
                  : {
                      bg: 'var(--bg-surface)',
                      border: 'var(--accent-indigo)',
                      icon: 'var(--accent-indigo)',
                    }

            return (
              <div
                key={item.id}
                role="status"
                className="pointer-events-auto flex items-start justify-between gap-3 rounded-xl border px-4 py-3 text-sm shadow-lg"
                style={{
                  backgroundColor: palette.bg,
                  borderColor: palette.border,
                  color: 'var(--text-primary)',
                }}
              >
                <div className="flex items-start gap-2.5">
                  {item.variant === 'success' && (
                    <CheckCircle2
                      className="mt-0.5 h-4 w-4 shrink-0"
                      style={{ color: palette.icon }}
                    />
                  )}
                  {item.variant === 'error' && (
                    <XCircle
                      className="mt-0.5 h-4 w-4 shrink-0"
                      style={{ color: palette.icon }}
                    />
                  )}
                  {item.variant === 'info' && (
                    <Info
                      className="mt-0.5 h-4 w-4 shrink-0"
                      style={{ color: palette.icon }}
                    />
                  )}
                  <span className="leading-snug font-medium">{item.message}</span>
                </div>
                <button
                  type="button"
                  onClick={() => dismiss(item.id)}
                  className="shrink-0 rounded-md p-0.5 transition-opacity hover:opacity-75 cursor-pointer"
                  style={{ color: 'var(--text-muted)' }}
                  aria-label="Dismiss notification"
                >
                  <X className="h-4 w-4" />
                </button>
              </div>
            )
          })}
        </div>
      )}
    </ToastContext.Provider>
  )
}

export function useToast(): ToastContextValue {
  const ctx = useContext(ToastContext)
  if (!ctx) {
    return {
      toast: {
        success: () => {},
        error: () => {},
        info: () => {},
      },
      dismiss: () => {},
    }
  }
  return ctx
}
