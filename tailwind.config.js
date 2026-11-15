/** @type {import('tailwindcss').Config} */

/** A colour token backed by a CSS variable (see src/renderer/theme.css), with Tailwind opacity. */
const v = (name) => `rgb(var(--ui-${name}) / <alpha-value>)`;

export default {
  content: [
    './src/renderer/index.html',
    './src/renderer/**/*.{js,ts,jsx,tsx}',
  ],
  theme: {
    extend: {
      /*
       * Semantic colour roles. Values live in src/renderer/theme.css and switch between the
       * dark and light palettes (graphite neutrals, teal for selection/focus, status colours
       * reserved for credential state). The GPUI app uses the same roles (apm-ui theme.rs).
       */
      colors: {
        ui: {
          sidebar: v('sidebar'),
          page: v('page'),
          panel: v('panel'),
          sunken: v('sunken'),
          raised: v('raised'),
          hover: v('hover'),
          border: v('border'),
          borderStrong: v('border-strong'),
          text: v('text'),
          textMuted: v('text-muted'),
          textFaint: v('text-faint'),
          primary: v('primary'),
          onPrimary: v('on-primary'),
          accent: v('accent'),
          accentHover: v('accent-hover'),
          onAccent: v('on-accent'),
          success: v('success'),
          warning: v('warning'),
          danger: v('danger'),
        },
      },
      fontFamily: {
        sans: ['"IBM Plex Sans"', 'system-ui', 'sans-serif'],
        mono: ['"IBM Plex Mono"', 'ui-monospace', 'monospace'],
      },
      borderRadius: {
        panel: '10px',
        card: '12px',
        button: '8px',
      },
      boxShadow: {
        // Depth comes from surface steps and hairlines; shadows only for floating layers.
        'ui-panel': 'var(--ui-shadow-panel)',
        'ui-card': 'var(--ui-shadow-card)',
        'ui-modal': 'var(--ui-shadow-modal)',
        'ui-accent': 'none',
        'ui-accent-hover': 'none',
      },
      transitionTimingFunction: {
        ui: 'cubic-bezier(0.22, 1, 0.36, 1)',
      },
      animation: {
        'modal-in': 'modalIn 200ms cubic-bezier(0.22, 1, 0.36, 1)',
      },
      keyframes: {
        modalIn: {
          '0%': { opacity: '0', transform: 'translateY(6px)' },
          '100%': { opacity: '1', transform: 'translateY(0)' },
        },
      },
    },
  },
  plugins: [],
};
