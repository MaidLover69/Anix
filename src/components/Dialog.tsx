import React from 'react';
import { Modal } from './ui/Modal';
import type { ModalSize } from './ui/Modal';

interface DialogProps {
  isOpen: boolean;
  onClose: () => void;
  title: string;
  children: React.ReactNode;
  /** A Tailwind max-width class, mapped to the nearest `Modal` size. */
  maxWidth?: string;
  /** Action row, right-aligned. */
  footer?: React.ReactNode;
  /** Portal target; by default the fullscreen element if any, else the document body. */
  container?: HTMLElement | null;
}

const SIZE_BY_MAX_WIDTH: Record<string, ModalSize> = {
  'max-w-xs': 'sm',
  'max-w-sm': 'sm',
  'max-w-md': 'md',
  'max-w-lg': 'lg',
  'max-w-xl': 'lg',
  'max-w-2xl': 'xl',
  'max-w-3xl': '2xl',
  'max-w-4xl': '2xl',
};

/** Kept for existing callers; new code should use `Modal` from `./ui`. */
export const Dialog: React.FC<DialogProps> = ({
  isOpen,
  onClose,
  title,
  children,
  maxWidth = 'max-w-lg',
  footer,
  container,
}) => (
  <Modal
    isOpen={isOpen}
    onClose={onClose}
    title={title}
    size={SIZE_BY_MAX_WIDTH[maxWidth] ?? 'lg'}
    footer={footer}
    container={container}
  >
    {children}
  </Modal>
);
