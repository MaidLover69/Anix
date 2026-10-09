import React, { useState, useRef, useEffect, useId } from 'react';
import { ChevronDown, Check } from 'lucide-react';
import clsx from 'clsx';

interface Option {
  value: string;
  label: string;
}

interface CustomSelectProps {
  value: string;
  onChange: (value: string) => void;
  options: (string | Option)[];
  placeholder?: string;
  className?: string;
  /** Which side of the trigger the menu opens on. */
  placement?: 'up' | 'down';
  size?: 'sm' | 'md';
  disabled?: boolean;
  'aria-label'?: string;
  'aria-labelledby'?: string;
}

export const CustomSelect: React.FC<CustomSelectProps> = ({
  value,
  onChange,
  options,
  placeholder = 'Select…',
  className = '',
  placement = 'down',
  size = 'md',
  disabled = false,
  ...aria
}) => {
  const [isOpen, setIsOpen] = useState(false);
  const [activeIndex, setActiveIndex] = useState(-1);
  const containerRef = useRef<HTMLDivElement>(null);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const listRef = useRef<HTMLDivElement>(null);
  const listId = useId();

  const normalizedOptions: Option[] = options.map((opt) =>
    typeof opt === 'string' ? { value: opt, label: opt } : opt
  );

  const selectedIndex = normalizedOptions.findIndex((opt) => opt.value === value);
  const selectedOption = selectedIndex === -1 ? undefined : normalizedOptions[selectedIndex];

  useEffect(() => {
    if (!isOpen) return;
    const handleClickOutside = (event: MouseEvent) => {
      if (containerRef.current && !containerRef.current.contains(event.target as Node)) {
        setIsOpen(false);
      }
    };
    document.addEventListener('mousedown', handleClickOutside);
    return () => document.removeEventListener('mousedown', handleClickOutside);
  }, [isOpen]);

  useEffect(() => {
    if (!isOpen || activeIndex < 0) return;
    listRef.current
      ?.querySelector<HTMLElement>(`[data-index="${activeIndex}"]`)
      ?.scrollIntoView({ block: 'nearest' });
  }, [isOpen, activeIndex]);

  const openMenu = () => {
    setActiveIndex(selectedIndex === -1 ? 0 : selectedIndex);
    setIsOpen(true);
  };

  const choose = (index: number) => {
    const option = normalizedOptions[index];
    if (!option) return;
    onChange(option.value);
    setIsOpen(false);
    triggerRef.current?.focus();
  };

  const handleKeyDown = (e: React.KeyboardEvent) => {
    const count = normalizedOptions.length;
    if (!isOpen) {
      if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
        e.preventDefault();
        openMenu();
      }
      return;
    }

    if (e.key === 'Tab') {
      setIsOpen(false);
      return;
    }

    const handled = ['Escape', 'ArrowDown', 'ArrowUp', 'Home', 'End', 'Enter', ' '];
    if (!handled.includes(e.key)) return;
    // While the menu is open these keys belong to it: an enclosing dialog must not
    // close on Escape, and the player must not seek or change volume.
    e.preventDefault();
    e.stopPropagation();

    switch (e.key) {
      case 'Escape':
        setIsOpen(false);
        break;
      case 'ArrowDown':
        if (count > 0) setActiveIndex((i) => (i + 1) % count);
        break;
      case 'ArrowUp':
        if (count > 0) setActiveIndex((i) => (i - 1 + count) % count);
        break;
      case 'Home':
        setActiveIndex(0);
        break;
      case 'End':
        setActiveIndex(count - 1);
        break;
      default:
        choose(activeIndex);
    }
  };

  return (
    <div ref={containerRef} className={`relative inline-block ${className}`} onKeyDown={handleKeyDown}>
      <button
        ref={triggerRef}
        type="button"
        disabled={disabled}
        aria-haspopup="listbox"
        aria-expanded={isOpen}
        aria-controls={isOpen ? listId : undefined}
        aria-label={aria['aria-label']}
        aria-labelledby={aria['aria-labelledby']}
        onClick={() => (isOpen ? setIsOpen(false) : openMenu())}
        className={clsx(
          'flex w-full min-w-[112px] items-center justify-between gap-2 rounded-md bg-bg-input px-3 text-[13px] font-medium text-text-primary cursor-pointer select-none transition-colors hover:bg-white/10 disabled:opacity-40 disabled:pointer-events-none',
          size === 'sm' ? 'h-8' : 'h-9'
        )}
      >
        <span className="truncate">{selectedOption ? selectedOption.label : placeholder}</span>
        <ChevronDown aria-hidden="true" className="h-4 w-4 flex-shrink-0 text-text-muted" />
      </button>

      {isOpen && (
        <div
          ref={listRef}
          id={listId}
          role="listbox"
          className={clsx(
            'absolute right-0 z-50 max-h-60 w-full min-w-[160px] overflow-y-auto rounded-xl border border-border-strong bg-bg-elevated p-1 shadow-[0_8px_24px_rgba(0,0,0,0.5)] animate-fade-in',
            placement === 'up' ? 'bottom-full mb-1' : 'top-full mt-1'
          )}
        >
          {normalizedOptions.map((opt, index) => {
            const isSelected = opt.value === value;
            return (
              <div
                key={opt.value}
                role="option"
                aria-selected={isSelected}
                data-index={index}
                onClick={() => choose(index)}
                onMouseMove={() => setActiveIndex(index)}
                className={clsx(
                  'flex h-8 items-center justify-between gap-2 rounded-md px-3 text-[13px] cursor-pointer select-none',
                  isSelected
                    ? 'font-medium text-accent-300'
                    : index === activeIndex
                    ? 'text-text-primary'
                    : 'text-text-secondary',
                  index === activeIndex && 'bg-white/10'
                )}
              >
                <span className="truncate">{opt.label}</span>
                {isSelected && <Check aria-hidden="true" className="h-4 w-4 flex-shrink-0" />}
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
};

export default CustomSelect;
