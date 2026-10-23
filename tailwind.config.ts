import type { Config } from 'tailwindcss';

export default {
  content: ['./index.html', './src/**/*.{ts,tsx}'],
  theme: {
    extend: {
      colors: {
        canvas: 'var(--canvas)',
        card: 'var(--card)',
        cpu: 'var(--cpu)',
        memory: 'var(--memory)',
        gpu: 'var(--gpu)',
        disk: 'var(--disk)',
        network: 'var(--network)',
        power: 'var(--power)',
      },
      fontFamily: { sans: ['Inter', 'sans-serif'] },
    },
  },
  plugins: [],
} satisfies Config;
