import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { defineConfig, transformerDirectives, transformerVariantGroup } from 'unocss'
import presetWind4 from '@unocss/preset-wind4'
import presetIcons from '@unocss/preset-icons'
import { icons as carbonIcons } from '@iconify-json/carbon'

// 自定义图标（i-app-*）：SVG 源放 src/assets/icons/，须 fill="currentColor" 才能跟随文字色
const appIconDir = fileURLToPath(new URL('./src/assets/icons', import.meta.url))
const appIcon = (name: string) => () => readFileSync(`${appIconDir}/${name}.svg`, 'utf-8')

export default defineConfig({
  presets: [
    presetWind4(),
    presetIcons({
      scale: 1.2,
      extraProperties: {
        'display': 'inline-block',
        'vertical-align': 'middle',
      },
      collections: {
        carbon: () => carbonIcons,
        app: {
          horse: appIcon('horse'),
          chrome: appIcon('chrome'),
        },
      },
    }),
  ],
  dark: 'class',
  transformers: [
    transformerDirectives(),
    transformerVariantGroup(),
  ],
  safelist: ['i-carbon-sun', 'i-carbon-moon', 'i-carbon-renew'],
  shortcuts: [
    ['center', 'flex justify-center items-center'],
    ['flex-center', 'flex items-center justify-center'],
    ['content-area', 'max-w-280 mx-auto w-full'],
  ],
  rules: [
    // paper 三层棕偏阴影（铁律：阴影只用这两枚 token）
    ['shadow-paper', { 'box-shadow': 'var(--shadow-paper)' }],
    ['shadow-paper-lifted', { 'box-shadow': 'var(--shadow-paper-lifted)' }],
  ],
  theme: {
    // paper 语义 token（shadcn 命名）：颜色只存在于 CSS 变量层，此处零字面量
    colors: {
      background: 'var(--background)',
      foreground: 'var(--foreground)',
      card: {
        DEFAULT: 'var(--card)',
        foreground: 'var(--card-foreground)',
      },
      popover: {
        DEFAULT: 'var(--popover)',
        foreground: 'var(--popover-foreground)',
      },
      primary: {
        DEFAULT: 'var(--primary)',
        foreground: 'var(--primary-foreground)',
      },
      secondary: {
        DEFAULT: 'var(--secondary)',
        foreground: 'var(--secondary-foreground)',
      },
      muted: {
        DEFAULT: 'var(--muted)',
        foreground: 'var(--muted-foreground)',
      },
      accent: {
        DEFAULT: 'var(--accent)',
        foreground: 'var(--accent-foreground)',
      },
      destructive: {
        DEFAULT: 'var(--destructive)',
        foreground: 'var(--destructive-foreground)',
      },
      border: 'var(--border)',
      input: 'var(--input)',
      ring: 'var(--ring)',
      // 项目扩展 token（见 src/styles/paper/extends.css）
      claude: 'var(--claude)',
      codex: 'var(--codex)',
    },
    font: {
      sans: 'var(--font-sans)',
      mono: 'var(--font-mono)',
    },
    radius: {
      DEFAULT: 'var(--radius)',
    },
  },
})
