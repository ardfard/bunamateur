import React from 'react'
import { AlertTriangle, Home, RefreshCw } from 'lucide-react'

interface ErrorBoundaryProps {
  children: React.ReactNode
}

interface ErrorBoundaryState {
  hasError: boolean
  errorMessage: string | null
}

export class ErrorBoundary extends React.Component<ErrorBoundaryProps, ErrorBoundaryState> {
  constructor(props: ErrorBoundaryProps) {
    super(props)
    this.state = { hasError: false, errorMessage: null }
  }

  static getDerivedStateFromError(error: Error): ErrorBoundaryState {
    return {
      hasError: true,
      errorMessage: error?.message || 'An unexpected rendering error occurred.',
    }
  }

  componentDidCatch(error: Error, info: React.ErrorInfo) {
    console.error('BunAmateur ErrorBoundary caught error:', error, info)
  }

  handleReload = () => {
    window.location.reload()
  }

  handleGoHome = () => {
    if (typeof window !== 'undefined' && window.history) {
      window.history.pushState(null, '', '/')
    }
    this.setState({ hasError: false, errorMessage: null })
  }

  render() {
    if (!this.state.hasError) {
      return this.props.children
    }

    return (
      <div
        className="flex min-h-screen items-center justify-center p-6"
        style={{
          backgroundColor: 'var(--bg-canvas)',
          color: 'var(--text-primary)',
        }}
      >
        <div
          className="flex w-full max-w-md flex-col items-center gap-5 rounded-2xl border p-8 text-center shadow-sm"
          style={{
            backgroundColor: 'var(--bg-surface)',
            borderColor: 'var(--border-subtle)',
          }}
        >
          <div
            className="flex h-12 w-12 items-center justify-center rounded-xl"
            style={{
              backgroundColor: 'var(--accent-vermilion-soft)',
              color: 'var(--accent-vermilion)',
            }}
          >
            <AlertTriangle className="h-6 w-6" />
          </div>

          <div className="flex flex-col gap-1.5">
            <h1 className="font-jp text-xl font-bold tracking-tight">
              エラーが発生しました
            </h1>
            <p className="text-sm" style={{ color: 'var(--text-secondary)' }}>
              Something unexpected interrupted this view. Your study progress is safely stored.
            </p>
          </div>

          {this.state.errorMessage && (
            <div
              className="font-mono-code w-full rounded-xl border px-3 py-2 text-left text-xs break-words"
              style={{
                backgroundColor: 'var(--bg-canvas)',
                borderColor: 'var(--border-subtle)',
                color: 'var(--text-muted)',
              }}
            >
              {this.state.errorMessage}
            </div>
          )}

          <div className="flex w-full flex-wrap items-center justify-center gap-3 pt-1">
            <button
              type="button"
              onClick={this.handleReload}
              className="flex items-center gap-1.5 rounded-xl px-4 py-2.5 text-sm font-semibold text-white cursor-pointer"
              style={{ backgroundColor: 'var(--accent-vermilion)' }}
            >
              <RefreshCw className="h-4 w-4" />
              Reload Page
            </button>
            <button
              type="button"
              onClick={this.handleGoHome}
              className="flex items-center gap-1.5 rounded-xl border px-4 py-2.5 text-sm font-semibold cursor-pointer"
              style={{
                backgroundColor: 'var(--bg-subtle)',
                borderColor: 'var(--border-subtle)',
                color: 'var(--text-primary)',
              }}
            >
              <Home className="h-4 w-4" />
              Go Home
            </button>
          </div>
        </div>
      </div>
    )
  }
}
