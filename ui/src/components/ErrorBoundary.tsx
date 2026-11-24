import { Component, ErrorInfo, ReactNode } from 'react'

interface Props {
  /** Pane name ("viewer", "editor", …). With one, a crash replaces only that
   *  pane and "Try again" re-mounts it; the session lives in the store, so it
   *  survives. Without one, the whole window is replaced (last resort). */
  name?: string
  children: ReactNode
}

interface State { error: Error | null }

export default class ErrorBoundary extends Component<Props, State> {
  state: State = { error: null }

  static getDerivedStateFromError(error: Error): State {
    return { error }
  }

  componentDidCatch(error: Error, info: ErrorInfo) {
    console.error(`[ErrorBoundary${this.props.name ? `: ${this.props.name}` : ''}]`, error, info.componentStack)
  }

  render() {
    const { error } = this.state
    if (!error) return this.props.children

    if (this.props.name) {
      return (
        <div role="alert" className="flex h-full w-full flex-col items-center justify-center gap-3 bg-base-100 p-4 text-center">
          <p className="text-sm text-base-content/70">
            The {this.props.name} stopped working: {error.message}
          </p>
          <button className="btn btn-sm btn-neutral" onClick={() => this.setState({ error: null })}>
            Try again
          </button>
        </div>
      )
    }
    return (
      <div role="alert" className="flex h-screen w-screen flex-col items-center justify-center gap-4 bg-base-100">
        <p className="text-sm text-base-content/70">Something went wrong. Reload the window to continue.</p>
        <button className="btn btn-sm btn-neutral" onClick={() => window.location.reload()}>
          Reload
        </button>
      </div>
    )
  }
}
