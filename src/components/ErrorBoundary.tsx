import { Component, ErrorInfo, ReactNode } from 'react';
import { ErrorState } from './ui/ErrorState';
import { t } from '../i18n';

interface Props {
  children: ReactNode;
  fallback?: ReactNode;
  /** When this value changes (for example the route), a caught error is cleared. */
  resetKey?: string;
  /** The "Playful touches" setting, read by the parent since a class can't call hooks. */
  playful?: boolean;
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
            title={this.props.playful ? t('errorBoundary.titlePlayful') : t('errorBoundary.title')}
            message={this.state.error?.message || t('errorBoundary.unexpected')}
            retryLabel={this.props.playful ? t('errorBoundary.retryPlayful') : t('common.tryAgain')}
            mood="dizzy"
            onRetry={() => this.setState({ hasError: false, error: undefined })}
          />
        )
      );
    }
    return this.props.children;
  }
}
