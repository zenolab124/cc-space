export type ThemeAppearance = 'light' | 'dark'
export type ThemeSourceKind = 'local' | 'community'

export interface ThemeSource {
  kind: ThemeSourceKind
  issue?: number
}

export interface ThemeColors {
  background: string
  foreground: string
  card: string
  cardForeground: string
  popover: string
  popoverForeground: string
  primary: string
  primaryForeground: string
  secondary: string
  secondaryForeground: string
  muted: string
  mutedForeground: string
  accent: string
  accentForeground: string
  destructive: string
  destructiveForeground: string
  border: string
  input: string
  ring: string
  claude: string
  codex: string
  tag: string
  tagForeground: string
  visualBorder: string
  visualWarm: string
  visualCool: string
  visualRed: string
  visualGreen: string
}

export interface ThemeDefinition {
  schemaVersion: 1
  id: string
  name: string
  author: string
  description: string
  version: string
  appearance: ThemeAppearance
  source: ThemeSource
  colors: ThemeColors
  metrics: {
    radius: number
    fontScale: number
    lineHeight: number
    shadow: {
      color: string
      opacity: number
      y: number
      blur: number
    }
  }
  atmosphere: {
    tint: string
    noise: number
    vignette: number
  }
}

export interface ThemeValidationIssue {
  field: string
  message: string
  kind: 'schema' | 'unsafe' | 'contrast' | 'range' | string
}

export interface ThemeValidationReport {
  valid: boolean
  issues: ThemeValidationIssue[]
}

export interface ThemePreview {
  previewId: string
  theme: ThemeDefinition
  validation: ThemeValidationReport
  createdAt: string
  baseThemeId?: string
}

export interface ThemeLibrary {
  themes: ThemeDefinition[]
  previews: ThemePreview[]
  invalidEntries: string[]
}
