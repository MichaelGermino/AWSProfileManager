/** @type {import('tailwindcss').Config} */
export default {
  content: [
    './src/renderer/index.html',
    './src/renderer/**/*.{js,ts,jsx,tsx}',
  ],
  theme: {
    extend: {
      colors: {
        discord: {
          dark: '#313338',
          darker: '#2b2d31',
          darkest: '#1e1f22',
          content: '#232428',
          sidebar: '#1e1f22',
          panel: '#2b2d31',
          panelHover: '#313338',
          accent: '#5865f2',
          accentHover: '#4752c4',
          accentGlow: 'rgba(88, 101, 242, 0.35)',
          text: '#f2f3f5',
          textMuted: '#b5bac1',
          textMutedHover: '#dbdee1',
          success: '#23a559',
          danger: '#f23f43',
          warning: '#f0b232',
          border: '#3f4147',
          borderLight: '#4e5058',
        },
      },
      borderRadius: {
        panel: '8px',
        card: '14px',
        button: '8px',
      },
      boxShadow: {
        'discord-panel': '0 2px 8px rgba(0,0,0,0.3), 0 1px 0 rgba(255,255,255,0.03) inset',
        'discord-card': '0 4px 24px rgba(0,0,0,0.4)',
        'discord-modal': '0 24px 48px rgba(0,0,0,0.5), 0 0 0 1px rgba(255,255,255,0.05)',
        'discord-accent': '0 0 0 1px rgba(88,101,242,0.4), 0 2px 8px rgba(88,101,242,0.2)',
        'discord-accent-hover': '0 0 0 1px rgba(88,101,242,0.5), 0 4px 16px rgba(88,101,242,0.3)',
      },
      animation: {
        'modal-in': 'modalIn 0.2s ease-out',
      },
      keyframes: {
        modalIn: {
          '0%': { opacity: '0', transform: 'scale(0.96)' },
          '100%': { opacity: '1', transform: 'scale(1)' },
        },
      },
    },
  },
  plugins: [],
};
