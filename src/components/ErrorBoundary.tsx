import { Component, ErrorInfo, ReactNode } from 'react';
import { ErrorState } from './ui/ErrorState';

interface Props {
  children: ReactNode;
  fallback?: ReactNode;
  /** When this value changes (for example the route), a caught error is cleared. */
  resetKey?: string;
}

interface State {
  hasError: boolean;
  error?: Error;
}

export class ErrorBoundary extends Component<Props, State> {
  state: State = { hasError: false };

  static getDerivedStateFromError(error: Error): State {
    return { hasError: true, error };
  }

  componentDidCatch(error: Error, info: ErrorInfo) {
    console.error('[ErrorBoundary]', error, info);
  }

  componentDidUpdate(prevProps: Props) {
    if (this.state.hasError && prevProps.resetKey !== this.props.resetKey) {
      this.setState({ hasError: false, error: undefined });
    }
  }

  render() {
    if (this.state.hasError) {
      return (
        this.props.fallback ?? (
          <ErrorState
            className="h-full min-h-[300px]"
            title="This screen ran into a problem"
            message={this.state.error?.message || 'An unexpected error occurred.'}
            retryLabel="Try again"
            onRetry={() => this.setState({ hasError: false, error: undefined })}
          />
        )
      );
    }
    return this.props.children;
  }
}
