import React from 'react';
import clsx from 'clsx';

export type ButtonVariant = 'primary' | 'secondary' | 'neutral' | 'ghost' | 'danger';
export type ButtonSize = 'sm' | 'md';

type NativeButtonProps = Omit<React.ButtonHTMLAttributes<HTMLButtonElement>, 'aria-label'>;

interface CommonProps extends NativeButtonProps {
  variant?: ButtonVariant;
  size?: ButtonSize;
}

type LabelledButtonProps = CommonProps & { icon?: false; 'aria-label'?: string };
// An icon-only button has no visible text, so it must be named for screen readers.
type IconButtonProps = CommonProps & { icon: true; 'aria-label': string };

export type ButtonProps = LabelledButtonProps | IconButtonProps;

const VARIANT_CLASSES: Record<ButtonVariant, string> = {
  primary: 'bg-accent-600 enabled:hover:bg-accent-700 text-on-accent',
  secondary: 'bg-accent-tint enabled:hover:bg-accent-500/20 text-accent-300 border border-accent-500/30',
  // A quiet filled button for secondary actions that still need a visible shape.
  neutral: 'bg-ink/[0.08] enabled:hover:bg-ink/[0.14] text-text-primary',
  ghost: 'text-text-secondary enabled:hover:text-text-primary enabled:hover:bg-ink/10',
  danger: 'bg-state-error/15 enabled:hover:bg-state-error/25 text-state-error',
};

const SIZE_CLASSES: Record<ButtonSize, string> = {
  sm: 'h-8 px-3',
  md: 'h-9 px-4',
};

const ICON_SIZE_CLASSES: Record<ButtonSize, string> = {
  sm: 'h-8 w-8',
  md: 'h-9 w-9',
};

export const Button = React.forwardRef<HTMLButtonElement, ButtonProps>(
  ({ variant = 'secondary', size = 'md', icon = false, className, type = 'button', ...rest }, ref) => (
    <button
      ref={ref}
      type={type}
      data-variant={variant}
      className={clsx(
        'ui-btn inline-flex flex-shrink-0 items-center justify-center gap-2 rounded-md text-[13px] font-medium whitespace-nowrap select-none cursor-pointer transition-colors enabled:active:opacity-80 disabled:opacity-40 disabled:cursor-not-allowed',
        VARIANT_CLASSES[variant],
        icon ? ICON_SIZE_CLASSES[size] : SIZE_CLASSES[size],
        className
      )}
      {...rest}
    />
  )
);

Button.displayName = 'Button';
