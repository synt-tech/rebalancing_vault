/** @type {import('tailwindcss').Config} */
export default {
  content: [
    "./index.html",
    "./src/**/*.{js,ts,jsx,tsx}",
  ],
  theme: {
    extend: {
      fontSize: {
        'xs': ['13px', { lineHeight: '1.4' }],
        'sm': ['15px', { lineHeight: '1.5' }],
      },
      colors: {
        'synt_tech_dark': '#121212',
        'synt_tech_gold': '#C1A100',
        'synt_tech_purple': '#a020f0',
        'synt_tech_darker': '#0a0a0a',
        'synt_tech_card': '#1a1a1a',
        'synt_tech_border': '#4a2040',
      },
      fontFamily: {
        'sans': ['8-bit Operator', 'monospace'],
        'mono': ['8-bit Operator', 'monospace'],
        'pixel': ['8-bit Operator', 'monospace'],
      },
      animation: {
        'halogen-pulse': 'halogen-pulse 4s infinite ease-in-out',
        'slow-rotate': 'slow-rotate 21600s linear infinite',
      },
      keyframes: {
        'halogen-pulse': {
          '0%, 100%': { 
            boxShadow: '0 0 10px #C1A100, inset 0 0 10px #C1A100',
            opacity: '0.7',
          },
          '50%': { 
            boxShadow: '0 0 30px #C1A100, inset 0 0 30px #C1A100',
            opacity: '1',
          },
        },
        'slow-rotate': {
          from: { transform: 'rotate(0deg)' },
          to: { transform: 'rotate(360deg)' },
        },
      },
    },
  },
  plugins: [],
}
