/** @type {import('tailwindcss').Config} */
export default {
  content: ['./index.html', './src/**/*.{ts,tsx}'],
  theme: {
    extend: {
      colors: {
        brand: {
          DEFAULT: '#FF8BA7',
          hover: '#FF5E8E',
          soft: '#FFF5F7',
        },
        accent: '#81D4FA',
        text: {
          main: '#5D4037',
          muted: '#A1887F',
        },
      },
      fontFamily: {
        brand: ['Outfit', 'PingFang SC', 'Microsoft YaHei', 'sans-serif'],
      },
      backdropBlur: {
        xs: '2px',
      },
    },
  },
  plugins: [],
};

