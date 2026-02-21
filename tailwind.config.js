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
          sidebar: '#2b2d31',
          panel: '#313338',
          accent: '#5865f2',
          accentHover: '#4752c4',
          text: '#f2f3f5',
          textMuted: '#b5bac1',
          success: '#23a559',
          danger: '#f23f43',
          warning: '#f0b232',
        },
      },
      borderRadius: {
        panel: '8px',
        button: '4px',
      },
    },
  },
  plugins: [],
};
