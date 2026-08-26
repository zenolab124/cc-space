<script setup lang="ts">
import { computed, onUnmounted, ref, watch } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { save } from '@tauri-apps/plugin-dialog'
import { useI18n } from 'vue-i18n'
import { themeCssVariables, useTheme } from '@/composables/useTheme'
import type { ThemeDefinition, ThemePreview } from '@/types/theme'
import type { ThemeMeta } from '@/composables/themeRegistry'
import { openExternalUrl } from '@/composables/useFileOpener'

interface SubmissionIdentity {
  mode: 'github' | 'anonymous'
  username: string | null
}

interface SubmissionPreview {
  identity: SubmissionIdentity
  title: string
  body: string
  themeJson: string
}

interface SubmissionResult {
  url: string
  mode: 'github' | 'anonymous'
}

const props = defineProps<{ active: boolean }>()
const { t } = useI18n()
const {
  config,
  lightThemes,
  darkThemes,
  localThemes,
  pendingPreviews,
  invalidThemeEntries,
  themeLibraryLoading,
  refreshThemeLibrary,
  setLightTheme,
  setDarkTheme,
  setThemeMode,
} = useTheme()

const dialog = ref<'editor' | 'rename' | 'delete' | 'share' | null>(null)
const request = ref('')
const currentPreview = ref<ThemePreview | null>(null)
const baseThemeId = ref<string | null>(null)
const generating = ref(false)
const actionLoading = ref(false)
const error = ref('')
const selectedTheme = ref<ThemeDefinition | null>(null)
const renameValue = ref('')
const replacementLight = ref('paper')
const replacementDark = ref('ink')
const publicName = ref('')
const submission = ref<SubmissionPreview | null>(null)
const submissionResult = ref<SubmissionResult | null>(null)
const shareConfirmed = ref(false)
let shareRefreshTimer = 0

const previewStyle = computed(() => currentPreview.value?.validation.valid
  ? themeCssVariables(currentPreview.value.theme)
  : {})
const replacementLightThemes = computed(() => lightThemes.value.filter(theme => theme.id !== selectedTheme.value?.id))
const replacementDarkThemes = computed(() => darkThemes.value.filter(theme => theme.id !== selectedTheme.value?.id))

function themeName(theme: ThemeMeta): string {
  if (theme.labelKey) return t(theme.labelKey)
  return theme.name ?? theme.id
}

function sourceLabel(theme: ThemeMeta): string {
  return t(`themeManager.source.${theme.source}`)
}

function openCreate() {
  dialog.value = 'editor'
  currentPreview.value = null
  baseThemeId.value = null
  request.value = ''
  error.value = ''
}

function openAdjust(theme: ThemeDefinition) {
  dialog.value = 'editor'
  currentPreview.value = null
  baseThemeId.value = theme.id
  request.value = ''
  error.value = ''
}

function reviewPreview(preview: ThemePreview) {
  dialog.value = 'editor'
  currentPreview.value = preview
  baseThemeId.value = preview.baseThemeId ?? null
  request.value = ''
  error.value = ''
}

async function generatePreview() {
  const prompt = request.value.trim()
  if (!prompt) return
  generating.value = true
  error.value = ''
  try {
    currentPreview.value = await invoke<ThemePreview>('theme_generate_preview', {
      request: prompt,
      previewId: currentPreview.value?.previewId ?? null,
      baseThemeId: baseThemeId.value,
    })
    request.value = ''
    await refreshThemeLibrary()
  } catch (cause) {
    error.value = String(cause)
  } finally {
    generating.value = false
  }
}

async function keepPreview() {
  const preview = currentPreview.value
  if (!preview?.validation.valid) return
  actionLoading.value = true
  error.value = ''
  try {
    const saved = await invoke<ThemeDefinition>('theme_save_preview', { previewId: preview.previewId })
    await refreshThemeLibrary()
    if (saved.appearance === 'dark') setDarkTheme(saved.id)
    else setLightTheme(saved.id)
    closeDialog()
  } catch (cause) {
    error.value = String(cause)
  } finally {
    actionLoading.value = false
  }
}

async function discardPreview() {
  if (currentPreview.value) {
    actionLoading.value = true
    try {
      await invoke('theme_discard_preview', { previewId: currentPreview.value.previewId })
      await refreshThemeLibrary()
    } catch (cause) {
      error.value = String(cause)
      actionLoading.value = false
      return
    }
    actionLoading.value = false
  }
  closeDialog()
}

function closeDialog(clearError = true) {
  dialog.value = null
  currentPreview.value = null
  baseThemeId.value = null
  selectedTheme.value = null
  submission.value = null
  submissionResult.value = null
  shareConfirmed.value = false
  if (clearError) error.value = ''
}

function openRename(theme: ThemeDefinition) {
  selectedTheme.value = theme
  renameValue.value = theme.name
  error.value = ''
  dialog.value = 'rename'
}

async function renameTheme() {
  if (!selectedTheme.value || !renameValue.value.trim()) return
  actionLoading.value = true
  try {
    await invoke('theme_rename', { themeId: selectedTheme.value.id, name: renameValue.value.trim() })
    await refreshThemeLibrary()
    closeDialog()
  } catch (cause) {
    error.value = String(cause)
  } finally {
    actionLoading.value = false
  }
}

function openDelete(theme: ThemeDefinition) {
  selectedTheme.value = theme
  replacementLight.value = theme.appearance === 'light' && config.value.lightTheme === theme.id ? 'paper' : config.value.lightTheme
  replacementDark.value = theme.appearance === 'dark' && config.value.darkTheme === theme.id ? 'ink' : config.value.darkTheme
  error.value = ''
  dialog.value = 'delete'
}

async function deleteTheme() {
  const theme = selectedTheme.value
  if (!theme) return
  actionLoading.value = true
  try {
    await invoke('theme_delete', { themeId: theme.id })
    if (config.value.lightTheme === theme.id) setLightTheme(replacementLight.value)
    if (config.value.darkTheme === theme.id) setDarkTheme(replacementDark.value)
    await refreshThemeLibrary()
    closeDialog()
  } catch (cause) {
    error.value = String(cause)
  } finally {
    actionLoading.value = false
  }
}

async function prepareShare(): Promise<boolean> {
  const theme = selectedTheme.value
  if (!theme) return false
  actionLoading.value = true
  error.value = ''
  try {
    submission.value = await invoke<SubmissionPreview>('theme_prepare_submission', {
      themeId: theme.id,
      publicName: publicName.value.trim() || null,
    })
    shareConfirmed.value = false
    return true
  } catch (cause) {
    error.value = String(cause)
    return false
  } finally {
    actionLoading.value = false
  }
}

function openShare(theme: ThemeDefinition) {
  selectedTheme.value = theme
  publicName.value = ''
  submission.value = null
  submissionResult.value = null
  shareConfirmed.value = false
  error.value = ''
  dialog.value = 'share'
  void prepareShare()
}

watch(publicName, () => {
  if (dialog.value !== 'share' || submission.value?.identity.mode !== 'anonymous') return
  shareConfirmed.value = false
  window.clearTimeout(shareRefreshTimer)
  shareRefreshTimer = window.setTimeout(() => { void prepareShare() }, 300)
})

async function submitTheme() {
  const theme = selectedTheme.value
  const prepared = submission.value
  if (!theme || !prepared || !shareConfirmed.value) return
  actionLoading.value = true
  error.value = ''
  try {
    submissionResult.value = await invoke<SubmissionResult>('theme_submit', {
      themeId: theme.id,
      publicName: publicName.value.trim() || null,
      expectedMode: prepared.identity.mode,
      expectedUsername: prepared.identity.username,
      expectedBody: prepared.body,
    })
  } catch (cause) {
    const message = String(cause)
    shareConfirmed.value = false
    await prepareShare()
    error.value = message
  } finally {
    actionLoading.value = false
  }
}

async function exportSubmission() {
  const theme = selectedTheme.value
  if (!theme) return
  const path = await save({
    defaultPath: `${theme.id}-theme-submission.md`,
    filters: [{ name: 'Markdown', extensions: ['md'] }],
  })
  if (!path) return
  try {
    await invoke('theme_export_submission', {
      themeId: theme.id,
      publicName: submission.value?.identity.mode === 'github'
        ? `@${submission.value.identity.username}`
        : (publicName.value.trim() || null),
      anonymous: submission.value?.identity.mode === 'anonymous',
      path,
    })
  } catch (cause) {
    error.value = String(cause)
  }
}

watch(() => props.active, active => {
  if (active) void refreshThemeLibrary()
}, { immediate: true })
onUnmounted(() => { window.clearTimeout(shareRefreshTimer) })
</script>

<template>
  <div class="theme-manager">
    <div class="theme-manager-mode">
      <label class="setting-label" for="theme-mode">{{ $t('themeManager.mode') }}</label>
      <select
        id="theme-mode"
        class="form-select"
        :value="config.mode"
        @change="setThemeMode(($event.target as HTMLSelectElement).value as 'system' | 'light' | 'dark')"
      >
        <option value="system">{{ $t('themeManager.modeSystem') }}</option>
        <option value="light">{{ $t('themeManager.modeLight') }}</option>
        <option value="dark">{{ $t('themeManager.modeDark') }}</option>
      </select>
    </div>

    <div class="appearance-theme-grid">
      <label class="appearance-field">
        <span class="appearance-field-heading">
          <span class="setting-label">{{ $t('settings.themeLight') }}</span>
          <span class="appearance-field-note">{{ $t('settings.themeLightHint') }}</span>
        </span>
        <select class="form-select" :value="config.lightTheme" @change="setLightTheme(($event.target as HTMLSelectElement).value)">
          <option v-for="theme in lightThemes" :key="theme.id" :value="theme.id">
            {{ themeName(theme) }} · {{ sourceLabel(theme) }}
          </option>
        </select>
      </label>
      <label class="appearance-field">
        <span class="appearance-field-heading">
          <span class="setting-label">{{ $t('settings.themeDark') }}</span>
          <span class="appearance-field-note">{{ $t('settings.themeDarkHint') }}</span>
        </span>
        <select class="form-select" :value="config.darkTheme" @change="setDarkTheme(($event.target as HTMLSelectElement).value)">
          <option v-for="theme in darkThemes" :key="theme.id" :value="theme.id">
            {{ themeName(theme) }} · {{ sourceLabel(theme) }}
          </option>
        </select>
      </label>
    </div>

    <div class="theme-manager-actions">
      <button type="button" class="appearance-primary-button" @click="openCreate">
        <span class="i-carbon-bot" />{{ $t('themeManager.createWithAi') }}
      </button>
      <span class="setting-hint">{{ $t('themeManager.aiOnlyHint') }}</span>
    </div>

    <div v-if="pendingPreviews.length" class="theme-pending">
      <strong>{{ $t('themeManager.pendingTitle') }}</strong>
      <button v-for="preview in pendingPreviews" :key="preview.previewId" type="button" @click="reviewPreview(preview)">
        <span :class="preview.validation.valid ? 'i-carbon-checkmark-outline' : 'i-carbon-warning-alt'" />
        {{ preview.theme.name }}
      </button>
    </div>

    <div v-if="localThemes.length" class="theme-local-list">
      <div v-for="theme in localThemes" :key="theme.id" class="theme-local-row">
        <span class="theme-swatch" :style="{ background: theme.colors.primary, borderColor: theme.colors.border }" />
        <span class="theme-local-copy">
          <strong>{{ theme.name }}</strong>
          <small>{{ theme.appearance === 'dark' ? $t('themeManager.dark') : $t('themeManager.light') }} · {{ theme.author }}</small>
        </span>
        <span class="theme-local-actions">
          <button type="button" v-tooltip="$t('themeManager.adjustWithAi')" @click="openAdjust(theme)"><span class="i-carbon-bot" /></button>
          <button type="button" v-tooltip="$t('common.edit')" @click="openRename(theme)"><span class="i-carbon-edit" /></button>
          <button type="button" v-tooltip="$t('themeManager.share')" @click="openShare(theme)"><span class="i-carbon-share" /></button>
          <button type="button" v-tooltip="$t('common.delete')" @click="openDelete(theme)"><span class="i-carbon-trash-can" /></button>
        </span>
      </div>
    </div>
    <p v-else-if="!themeLibraryLoading" class="setting-hint">{{ $t('themeManager.noCustomThemes') }}</p>
    <p v-if="invalidThemeEntries.length" class="theme-manager-warning">
      {{ $t('themeManager.invalidThemes', { count: invalidThemeEntries.length }) }}
    </p>

    <Teleport to="body">
      <div v-if="dialog" class="theme-dialog-backdrop" role="presentation">
        <section class="theme-dialog" role="dialog" aria-modal="true" :aria-label="$t('themeManager.dialogTitle')">
          <template v-if="dialog === 'editor'">
            <header>
              <div><h3>{{ $t('themeManager.editorTitle') }}</h3><p>{{ $t('themeManager.editorHint') }}</p></div>
              <span class="i-carbon-bot" />
            </header>
            <div v-if="currentPreview" class="theme-custom theme-preview-canvas" :style="previewStyle">
              <div class="theme-preview-sidebar"><span /><span /><span /></div>
              <div class="theme-preview-main">
                <div class="theme-preview-heading">{{ currentPreview.theme.name }}</div>
                <div class="theme-preview-card">
                  <strong>{{ $t('themeManager.previewCardTitle') }}</strong>
                  <p>{{ currentPreview.theme.description }}</p>
                  <div><button>{{ $t('common.confirm') }}</button><span>{{ $t('themeManager.previewMuted') }}</span></div>
                </div>
              </div>
            </div>
            <div v-if="currentPreview && !currentPreview.validation.valid" class="theme-validation">
              <strong>{{ $t('themeManager.validationFailed') }}</strong>
              <ul><li v-for="issue in currentPreview.validation.issues" :key="`${issue.field}:${issue.message}`"><code>{{ issue.field }}</code> — {{ issue.message }}</li></ul>
            </div>
            <label class="theme-request-label">
              <span>{{ currentPreview ? $t('themeManager.adjustPrompt') : $t('themeManager.createPrompt') }}</span>
              <textarea v-model="request" rows="4" :disabled="generating" :placeholder="$t('themeManager.promptPlaceholder')" />
            </label>
            <p v-if="error" class="theme-dialog-error">{{ error }}</p>
            <footer>
              <button type="button" class="appearance-cancel-button" :disabled="actionLoading" @click="discardPreview">{{ $t('themeManager.discard') }}</button>
              <span class="flex-1" />
              <button type="button" class="appearance-cancel-button" :disabled="generating || !request.trim()" @click="generatePreview">
                <span v-if="generating" class="i-carbon-renew animate-spin" />{{ currentPreview ? $t('themeManager.continueAdjusting') : $t('themeManager.generatePreview') }}
              </button>
              <button type="button" class="appearance-primary-button" :disabled="!currentPreview?.validation.valid || actionLoading" @click="keepPreview">{{ $t('themeManager.keepTheme') }}</button>
            </footer>
          </template>

          <template v-else-if="dialog === 'rename'">
            <header><div><h3>{{ $t('themeManager.renameTitle') }}</h3><p>{{ selectedTheme?.name }}</p></div></header>
            <input v-model="renameValue" class="form-input" maxlength="60" @keydown.enter="renameTheme" />
            <p v-if="error" class="theme-dialog-error">{{ error }}</p>
            <footer><span class="flex-1" /><button class="appearance-cancel-button" :disabled="actionLoading" @click="closeDialog()">{{ $t('common.cancel') }}</button><button class="appearance-primary-button" :disabled="actionLoading || !renameValue.trim()" @click="renameTheme">{{ $t('common.save') }}</button></footer>
          </template>

          <template v-else-if="dialog === 'delete'">
            <header><div><h3>{{ $t('themeManager.deleteTitle') }}</h3><p>{{ $t('themeManager.deleteHint', { name: selectedTheme?.name }) }}</p></div></header>
            <label v-if="selectedTheme?.appearance === 'light' && config.lightTheme === selectedTheme.id"><span>{{ $t('themeManager.replacementLight') }}</span><select v-model="replacementLight" class="form-select"><option v-for="theme in replacementLightThemes" :key="theme.id" :value="theme.id">{{ themeName(theme) }}</option></select></label>
            <label v-if="selectedTheme?.appearance === 'dark' && config.darkTheme === selectedTheme.id"><span>{{ $t('themeManager.replacementDark') }}</span><select v-model="replacementDark" class="form-select"><option v-for="theme in replacementDarkThemes" :key="theme.id" :value="theme.id">{{ themeName(theme) }}</option></select></label>
            <p v-if="error" class="theme-dialog-error">{{ error }}</p>
            <footer><span class="flex-1" /><button class="appearance-cancel-button" :disabled="actionLoading" @click="closeDialog()">{{ $t('common.cancel') }}</button><button class="theme-danger-button" :disabled="actionLoading" @click="deleteTheme">{{ $t('common.delete') }}</button></footer>
          </template>

          <template v-else-if="dialog === 'share'">
            <header><div><h3>{{ $t('themeManager.shareTitle') }}</h3><p>{{ $t('themeManager.shareHint') }}</p></div><span class="i-carbon-logo-github" /></header>
            <div v-if="submission" class="theme-share-identity">
              <span :class="submission.identity.mode === 'github' ? 'i-carbon-user-avatar' : 'i-carbon-user-anonymous'" />
              {{ submission.identity.mode === 'github' ? $t('themeManager.submitAsGithub', { name: submission.identity.username }) : $t('themeManager.submitAnonymously') }}
            </div>
            <label v-if="submission?.identity.mode === 'anonymous'" class="theme-request-label"><span>{{ $t('themeManager.publicName') }}</span><input v-model="publicName" class="form-input" maxlength="60" :placeholder="$t('themeManager.publicNamePlaceholder')" /></label>
            <details v-if="submission" open><summary>{{ $t('themeManager.publicPayload') }}</summary><pre>{{ submission.body }}</pre></details>
            <label v-if="submission && !submissionResult" class="theme-share-confirm"><input v-model="shareConfirmed" type="checkbox" />{{ $t('themeManager.publicConfirm') }}</label>
            <div v-if="submissionResult" class="theme-share-success"><span class="i-carbon-checkmark-filled" />{{ $t('themeManager.submitted') }}<button @click="openExternalUrl(submissionResult.url)">{{ $t('themeManager.openIssue') }}</button></div>
            <p v-if="error" class="theme-dialog-error">{{ error }}</p>
            <footer><button class="appearance-cancel-button" :disabled="actionLoading" @click="exportSubmission">{{ $t('themeManager.exportPackage') }}</button><span class="flex-1" /><button class="appearance-cancel-button" :disabled="actionLoading" @click="closeDialog()">{{ $t('common.close') }}</button><button v-if="!submissionResult" class="appearance-primary-button" :disabled="actionLoading || !submission || !shareConfirmed" @click="submitTheme">{{ $t('themeManager.submit') }}</button></footer>
          </template>
        </section>
      </div>
    </Teleport>
  </div>
</template>

<style scoped>
.theme-manager { display: flex; flex-direction: column; gap: 14px; }
.setting-label { color: var(--foreground); font-size: 11px; font-weight: 500; }
.setting-hint { color: var(--muted-foreground); font-size: 10px; line-height: 1.5; }
.appearance-theme-grid { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 16px; }
.appearance-field { display: flex; min-width: 0; flex-direction: column; }
.appearance-field-heading { display: flex; align-items: baseline; justify-content: space-between; gap: 10px; margin-bottom: 7px; }
.appearance-field-note { color: var(--muted-foreground); font-size: 10px; text-align: right; }
.appearance-primary-button, .appearance-cancel-button, .theme-danger-button { min-height: 30px; padding: 6px 10px; border-radius: var(--radius); font-size: 11px; }
.appearance-primary-button { color: var(--primary-foreground); background: var(--primary); }
.appearance-cancel-button { border: 1px solid var(--border); color: var(--muted-foreground); background: var(--background); }
.appearance-primary-button:disabled, .appearance-cancel-button:disabled, .theme-danger-button:disabled { cursor: not-allowed; opacity: .45; }
.theme-manager-mode { display: grid; grid-template-columns: 1fr 180px; align-items: center; gap: 12px; }
.theme-manager .form-select { min-height: 36px; background: var(--background); }
.theme-manager-actions { display: flex; align-items: center; gap: 10px; }
.theme-manager-actions button, .theme-dialog button { display: inline-flex; align-items: center; gap: 5px; }
.theme-pending { display: flex; flex-wrap: wrap; align-items: center; gap: 6px; padding-top: 10px; border-top: 1px solid var(--border); font-size: 11px; }
.theme-pending button { display: inline-flex; align-items: center; gap: 4px; padding: 4px 7px; border-radius: var(--radius); background: var(--muted); }
.theme-local-list { display: flex; flex-direction: column; border-top: 1px solid var(--border); }
.theme-local-row { display: flex; align-items: center; gap: 8px; min-height: 42px; border-bottom: 1px solid var(--border); }
.theme-swatch { width: 20px; height: 20px; flex: none; border: 1px solid; border-radius: 50%; }
.theme-local-copy { display: flex; min-width: 0; flex: 1; flex-direction: column; font-size: 11px; }
.theme-local-copy small { color: var(--muted-foreground); }
.theme-local-actions { display: flex; gap: 2px; }
.theme-local-actions button { width: 25px; height: 25px; display: grid; place-items: center; border-radius: var(--radius); color: var(--muted-foreground); }
.theme-local-actions button:hover { color: var(--foreground); background: var(--muted); }
.theme-manager-warning, .theme-dialog-error { color: var(--destructive); font-size: 11px; }
.theme-dialog-backdrop { position: fixed; inset: 0; z-index: 1000; display: grid; place-items: center; padding: 24px; background: rgb(0 0 0 / 0.48); backdrop-filter: blur(3px); }
.theme-dialog { width: min(720px, 92vw); max-height: min(820px, 90vh); overflow: auto; padding: 18px; border: 1px solid var(--border); border-radius: 8px; background: var(--card); box-shadow: var(--shadow-paper-lifted); }
.theme-dialog > header { display: flex; justify-content: space-between; gap: 16px; margin-bottom: 14px; }
.theme-dialog > header h3 { margin: 0 0 3px; font-size: 16px; }
.theme-dialog > header p { margin: 0; color: var(--muted-foreground); font-size: 11px; }
.theme-dialog > header > span { color: var(--primary); font-size: 22px; }
.theme-dialog footer { display: flex; align-items: center; gap: 8px; margin-top: 16px; padding-top: 12px; border-top: 1px solid var(--border); }
.theme-preview-canvas { display: grid; grid-template-columns: 72px 1fr; min-height: 220px; overflow: hidden; border: 1px solid var(--border); border-radius: var(--radius); color: var(--foreground); background: var(--background); box-shadow: var(--shadow-paper); }
.theme-preview-sidebar { display: flex; flex-direction: column; gap: 8px; padding: 14px 10px; background: var(--secondary); }
.theme-preview-sidebar span { height: 7px; border-radius: 4px; background: var(--muted-foreground); opacity: .55; }
.theme-preview-main { padding: 18px; }
.theme-preview-heading { margin-bottom: 12px; color: var(--primary); font-size: 17px; font-weight: 650; }
.theme-preview-card { padding: 14px; border: 1px solid var(--border); border-radius: var(--radius); color: var(--card-foreground); background: var(--card); box-shadow: var(--shadow-paper); }
.theme-preview-card p { color: var(--muted-foreground); }
.theme-preview-card div { display: flex; align-items: center; gap: 10px; }
.theme-preview-card button { padding: 5px 9px; border-radius: var(--radius); color: var(--primary-foreground); background: var(--primary); }
.theme-preview-card span { color: var(--muted-foreground); font-size: 11px; }
.theme-validation { margin-top: 10px; padding: 10px; border: 1px solid color-mix(in srgb, var(--destructive) 35%, var(--border)); border-radius: var(--radius); color: var(--destructive); font-size: 11px; background: color-mix(in srgb, var(--destructive) 6%, transparent); }
.theme-validation ul { margin: 6px 0 0; padding-left: 18px; }
.theme-request-label, .theme-dialog > label { display: flex; flex-direction: column; gap: 6px; margin-top: 12px; font-size: 11px; }
.theme-request-label textarea { resize: vertical; min-height: 88px; padding: 9px; border: 1px solid var(--border); border-radius: var(--radius); color: var(--foreground); background: var(--background); outline: none; }
.theme-request-label textarea:focus { border-color: var(--ring); }
.theme-danger-button { color: var(--destructive-foreground); background: var(--destructive); }
.theme-share-identity, .theme-share-success { display: flex; align-items: center; gap: 7px; padding: 9px 10px; border-radius: var(--radius); color: var(--primary); background: color-mix(in srgb, var(--primary) 8%, transparent); font-size: 11px; }
.theme-dialog details { margin-top: 12px; border: 1px solid var(--border); border-radius: var(--radius); }
.theme-dialog summary { padding: 8px 10px; font-size: 11px; cursor: pointer; }
.theme-dialog pre { max-height: 280px; overflow: auto; margin: 0; padding: 10px; border-top: 1px solid var(--border); white-space: pre-wrap; word-break: break-word; color: var(--muted-foreground); background: var(--background); font-size: 10px; }
.theme-share-confirm { flex-direction: row !important; align-items: flex-start; }
.theme-share-success button { margin-left: auto; text-decoration: underline; }
@media (max-width: 720px) { .theme-manager-mode, .appearance-theme-grid { grid-template-columns: 1fr; } }
</style>
