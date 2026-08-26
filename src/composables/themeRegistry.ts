import type { ThemeAppearance, ThemeDefinition } from '@/types/theme'

export interface ThemeMeta {
  id: string
  className: string
  isDark: boolean
  appearance: ThemeAppearance
  atmosphere: string | false
  icon: string
  labelKey?: string
  name?: string
  source: 'builtin' | 'community' | 'custom'
  definition?: ThemeDefinition
}

export const BUILTIN_THEMES: ThemeMeta[] = [
  {
    id: 'paper',
    className: 'theme-paper',
    isDark: false,
    appearance: 'light',
    atmosphere: 'paper-atmosphere',
    icon: 'i-carbon-sun',
    labelKey: 'theme.paper',
    source: 'builtin',
  },
  {
    id: 'ink',
    className: 'theme-ink',
    isDark: true,
    appearance: 'dark',
    atmosphere: false,
    icon: 'i-carbon-moon',
    labelKey: 'theme.ink',
    source: 'builtin',
  },
]

const communityModules = import.meta.glob<{ default: ThemeDefinition }>(
  '../themes/builtin/*.json',
  { eager: true },
)

export const COMMUNITY_THEMES: ThemeMeta[] = Object.values(communityModules).map(module => ({
  id: `community:${module.default.id}`,
  className: 'theme-custom',
  isDark: module.default.appearance === 'dark',
  appearance: module.default.appearance,
  atmosphere: 'custom-theme-atmosphere',
  icon: module.default.appearance === 'dark' ? 'i-carbon-moon' : 'i-carbon-sun',
  name: module.default.name,
  source: 'community',
  definition: module.default,
}))

export function themeMetaFromDefinition(
  definition: ThemeDefinition,
  source: 'community' | 'custom',
): ThemeMeta {
  return {
    id: source === 'community' ? `community:${definition.id}` : definition.id,
    className: 'theme-custom',
    isDark: definition.appearance === 'dark',
    appearance: definition.appearance,
    atmosphere: 'custom-theme-atmosphere',
    icon: definition.appearance === 'dark' ? 'i-carbon-moon' : 'i-carbon-sun',
    name: definition.name,
    source,
    definition,
  }
}
