import plugin from 'tailwindcss/plugin';

// Solid colours are stored as RGB channels so opacity modifiers (bg-accent-500/20) work.
const rgb = (variable) => `rgb(var(${variable}) / <alpha-value>)`;

// The hairline border is white at a low alpha; a modifier scales that alpha.
const hairline = ({ opacityValue }) =>
  opacityValue === undefined
    ? 'var(--border)'
    : `rgb(255 255 255 / calc(var(--border-alpha) * ${opacityValue}))`;

/** @type {import('tailwindcss').Config} */
export default {
  content: [
    "./index.html",
    "./src/**/*.{js,ts,jsx,tsx}",
  ],
  theme: {
    extend: {
      colors: {
        bg: {
          deepest: rgb('--bg-deepest'),
          page: rgb('--bg-page'),
          card: rgb('--bg-card'),
          elevated: rgb('--bg-elevated'),
          surface: rgb('--bg-surface'),
          input: rgb('--bg-input'),
        },
        border: {
          DEFAULT: hairline,
          subtle: hairline,
          strong: 'var(--border-strong)',
          focus: 'var(--border-focus)',
        },
        accent: {
          950: rgb('--accent-950'),
          900: rgb('--accent-900'),
          700: rgb('--accent-700'),
          600: rgb('--accent-600'),
          500: rgb('--accent-500'),
          400: rgb('--accent-400'),
          300: rgb('--accent-300'),
          200: rgb('--accent-200'),
          100: rgb('--accent-100'),
          tint: 'var(--accent-tint)',
        },
        text: {
          primary: 'var(--text-primary)',
          secondary: 'var(--text-secondary)',
          muted: 'var(--text-muted)',
          disabled: 'var(--text-disabled)',
        },
        state: {
          success: rgb('--success'),
          warning: rgb('--warning'),
          error: rgb('--error'),
        }
      },
      borderRadius: {
        sm: 'var(--radius-sm)',
        md: 'var(--radius-md)',
        lg: 'var(--radius-lg)',
        xl: 'var(--radius-xl)',
        '2xl': 'var(--radius-2xl)',
      },
      fontFamily: {
        sans: ['"Inter Variable"', 'Inter', 'Noto Sans', 'system-ui', 'sans-serif'],
      },
      backdropBlur: {
        xs: '2px',
      },
      animation: {
        'shimmer': 'shimmer 1.5s infinite linear',
        'fade-in': 'fadeIn 0.18s ease-out forwards',
        'fadeIn': 'fadeIn 0.18s ease-out forwards',
        'slide-up': 'slideUp 0.24s cubic-bezier(0.16, 1, 0.3, 1) forwards',
        'scale-up': 'scaleUp 0.18s cubic-bezier(0.16, 1, 0.3, 1) forwards',
        'slide-left': 'slideLeft 0.24s cubic-bezier(0.16, 1, 0.3, 1) forwards',
      },
      keyframes: {
        shimmer: {
          '0%': { backgroundPosition: '-200% 0' },
          '100%': { backgroundPosition: '200% 0' },
        },
        fadeIn: {
          '0%': { opacity: '0' },
          '100%': { opacity: '1' },
        },
        slideUp: {
          '0%': { opacity: '0', transform: 'translateY(12px)' },
          '100%': { opacity: '1', transform: 'translateY(0)' },
        },
        scaleUp: {
          '0%': { opacity: '0', transform: 'scale(0.96)' },
          '100%': { opacity: '1', transform: 'scale(1)' },
        },
        slideLeft: {
          '0%': { opacity: '0', transform: 'translateX(24px)' },
          '100%': { opacity: '1', transform: 'translateX(0)' },
        }
      }
    },
  },
  plugins: [
    plugin(({ addUtilities }) => {
      addUtilities({
        '.no-scrollbar': {
          'scrollbar-width': 'none',
          '&::-webkit-scrollbar': { display: 'none' },
        },
        '.scrollbar-thin': {
          'scrollbar-width': 'thin',
          '&::-webkit-scrollbar': { width: '4px', height: '4px' },
        },
      });
    }),
  ],
}
