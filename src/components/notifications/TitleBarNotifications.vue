<script setup lang="ts">
import { ref, computed, watch, onMounted, onUnmounted } from 'vue'
import { useI18n } from 'vue-i18n'
import {
  useNotifications,
  requestKindLabel,
  interactionKindLabel,
  isInteractiveKind,
  respondEngineToast,
  type PersistentToast,
} from '@/composables/useNotifications'
import {
  usePermissionRequests,
  isInteractiveTool,
} from '@/composables/usePermissionRequests'

const { t } = useI18n()
const {
  transients,
  persistentToasts,
  dismissError,
  goToSession,
  retryFromToast,
} = useNotifications()
const { respondRequest } = usePermissionRequests()

const current = computed<PersistentToast | null>(() => persistentToasts.value[0] ?? null)
const moreCount = computed(() => Math.max(0, persistentToasts.value.length - 1))
const transientText = computed(() => {
  if (current.value) return null
  const tr = transients.value[0]
  return tr ? (tr.sub ? `${tr.title} · ${tr.sub}` : tr.title) : null
})

function icon(toast: PersistentToast): string {
  if (toast.kind === 'error') return 'i-carbon-warning text-destructive'
  if (toast.kind === 'engine-interaction') {
    switch (toast.interaction.kind) {
      case 'question': return 'i-carbon-help text-accent'
      case 'plan': return 'i-carbon-task-approved text-accent'
      default: return 'i-carbon-locked text-accent'
    }
  }
  switch (toast.request.toolName) {
    case 'AskUserQuestion': return 'i-carbon-help text-accent'
    case 'ExitPlanMode':
    case 'EnterPlanMode': return 'i-carbon-task-approved text-accent'
    default: return 'i-carbon-locked text-accent'
  }
}

function label(toast: PersistentToast): string {
  if (toast.kind === 'permission') return requestKindLabel(toast.request.toolName)
  return toast.kind === 'engine-interaction'
    ? interactionKindLabel(toast.interaction.kind)
    : t('notification.errorStopped')
}

/** 提问/计划类必须去会话内的专用卡片作答——通知条上就地「允许」会送出空答 */
function needsSession(toast: PersistentToast): boolean {
  if (toast.kind === 'permission') return isInteractiveTool(toast.request.toolName)
  return toast.kind === 'engine-interaction' && isInteractiveKind(toast.interaction.kind)
}

async function onAllow(toast: PersistentToast) {
  if (toast.kind === 'permission')
    await respondRequest(toast.request.requestId, 'allow_once')
  else if (toast.kind === 'engine-interaction') await respondEngineToast(toast, 'allow')
}
async function onAllowSession(toast: PersistentToast) {
  if (toast.kind === 'permission')
    await respondRequest(toast.request.requestId, 'allow_session')
  else if (toast.kind === 'engine-interaction') await respondEngineToast(toast, 'allowSession')
}
async function onDeny(toast: PersistentToast) {
  if (toast.kind === 'permission')
    await respondRequest(toast.request.requestId, 'deny')
  else if (toast.kind === 'engine-interaction') await respondEngineToast(toast, 'deny')
}

// --- dropdown ---
const dropdownOpen = ref(false)
const dropdownRef = ref<HTMLElement>()

function toggleDropdown() {
  dropdownOpen.value = !dropdownOpen.value
}

function onOutside(e: MouseEvent) {
  if (dropdownRef.value && !dropdownRef.value.contains(e.target as Node)) {
    dropdownOpen.value = false
  }
}

watch(moreCount, (n) => { if (n <= 0) dropdownOpen.value = false })
onMounted(() => document.addEventListener('pointerdown', onOutside, true))
onUnmounted(() => document.removeEventListener('pointerdown', onOutside, true))
</script>

<template>
  <div
    class="flex-1 min-w-0 h-full flex items-center justify-center relative"
    data-tauri-drag-region
  >
    <!-- persistent notification strip -->
    <Transition name="notif-fade" mode="out-in">
      <div
        v-if="current"
        :key="current.key"
        class="notif-strip"
        :class="current.kind === 'error' ? 'strip-destructive' : 'strip-accent'"
      >
        <span class="w-3.5 h-3.5 shrink-0" :class="icon(current)" />
        <span class="truncate">
          <span class="font-medium">{{ label(current) }}</span>
          <span class="mx-1 opacity-40">·</span>
          <span>{{ current.title }}</span>
          <span class="mx-1 opacity-40">·</span>
          <span class="font-mono opacity-70">{{ current.sub }}</span>
        </span>

        <!-- actions -->
        <template v-if="current.kind !== 'error'">
          <template v-if="needsSession(current)">
            <button class="nb" @click="goToSession(current!.sessionId)">{{ $t('notification.goToSessionBrief') }}</button>
          </template>
          <template v-else>
            <button class="nb nb-primary" @click="onAllow(current!)">{{ $t('common.allow') }}</button>
            <button class="nb nb-outline" @click="onAllowSession(current!)" :title="$t('permission.allowSessionHint')">{{ $t('permission.allowSession') }}</button>
            <button class="nb nb-ghost" @click="onDeny(current!)">{{ $t('common.deny') }}</button>
          </template>
        </template>
        <template v-else>
          <button v-if="current.canRetry" class="nb nb-primary" @click="retryFromToast(current!.sessionId)">{{ $t('common.retry') }}</button>
          <button class="nb nb-ghost" @click="goToSession(current!.sessionId)">{{ $t('notification.goToSessionBrief') }}</button>
          <button class="nb nb-ghost" @click="dismissError(current!.sessionId)">
            <span class="i-carbon-close w-3 h-3" />
          </button>
        </template>

        <!-- +N -->
        <button
          v-if="moreCount > 0"
          class="nb nb-badge"
          @click.stop="toggleDropdown"
        >+{{ moreCount }}</button>
      </div>

      <!-- transient (only when no persistent) -->
      <div v-else-if="transientText" :key="'tr'" class="notif-strip strip-transient">
        <span class="i-carbon-checkmark-outline w-3.5 h-3.5 text-primary shrink-0" />
        <span class="truncate">{{ transientText }}</span>
      </div>
    </Transition>

    <!-- dropdown panel -->
    <Transition name="dd-fade">
      <div
        v-if="dropdownOpen && persistentToasts.length > 1"
        ref="dropdownRef"
        class="dd-panel"
      >
        <div
          v-for="toast in persistentToasts.slice(1)"
          :key="toast.key"
          class="dd-item"
          :class="toast.kind === 'error' ? 'dd-destructive' : 'dd-accent'"
        >
          <div class="flex items-center gap-1.5 min-w-0">
            <span class="w-3.5 h-3.5 shrink-0" :class="icon(toast)" />
            <span class="truncate text-foreground">{{ toast.title }}</span>
          </div>
          <div class="mt-0.5 font-mono text-muted-foreground truncate">{{ toast.sub }}</div>
          <div class="mt-1 flex items-center gap-1">
            <template v-if="toast.kind !== 'error'">
              <template v-if="needsSession(toast)">
                <button class="nb nb-primary" @click="goToSession(toast.sessionId)">{{ $t('notification.goToSessionBrief') }}</button>
              </template>
              <template v-else>
                <button class="nb nb-primary" @click="onAllow(toast)">{{ $t('common.allow') }}</button>
                <button class="nb nb-outline" @click="onAllowSession(toast)">{{ $t('permission.allowSession') }}</button>
                <button class="nb nb-ghost" @click="onDeny(toast)">{{ $t('common.deny') }}</button>
              </template>
            </template>
            <template v-else>
              <button v-if="toast.canRetry" class="nb nb-primary" @click="retryFromToast(toast.sessionId)">{{ $t('common.retry') }}</button>
              <button class="nb nb-ghost" @click="goToSession(toast.sessionId)">{{ $t('notification.goToSessionBrief') }}</button>
              <button class="nb nb-ghost" @click="dismissError(toast.sessionId)">
                <span class="i-carbon-close w-3 h-3" />
              </button>
            </template>
          </div>
        </div>
      </div>
    </Transition>
  </div>
</template>

<style scoped>
.notif-strip {
  display: flex;
  align-items: center;
  gap: 5px;
  max-width: 100%;
  min-width: 0;
  padding: 0 8px;
  height: 22px;
  border-radius: 4px;
  font-size: 11px;
  white-space: nowrap;
  color: var(--foreground);
}
.strip-accent {
  background: color-mix(in srgb, var(--accent) 12%, var(--card));
  border: 1px solid var(--accent);
}
.strip-destructive {
  background: hsl(var(--destructive-hsl, 0 84% 60%) / 0.08);
  border: 1px solid var(--destructive);
}
.strip-transient {
  background: var(--secondary);
  border: 1px solid var(--border);
  color: var(--muted-foreground);
}

/* notification buttons */
.nb {
  display: inline-flex;
  align-items: center;
  gap: 2px;
  padding: 1px 6px;
  border-radius: 3px;
  font-size: 10px;
  font-weight: 500;
  line-height: 1.4;
  cursor: pointer;
  border: 1px solid transparent;
  white-space: nowrap;
  flex-shrink: 0;
}
.nb-primary {
  background: var(--primary);
  color: var(--primary-foreground);
}
.nb-primary:hover { box-shadow: var(--shadow-paper); }
.nb-outline {
  border-color: var(--primary);
  color: var(--primary);
}
.nb-outline:hover { background: var(--secondary); }
.nb-ghost {
  border-color: var(--border);
  color: var(--muted-foreground);
}
.nb-ghost:hover { background: var(--muted); }
.nb-badge {
  background: var(--muted);
  color: var(--muted-foreground);
  font-size: 9px;
  padding: 1px 4px;
  border-radius: 3px;
}
.nb-badge:hover { background: var(--secondary); color: var(--foreground); }

/* dropdown */
.dd-panel {
  position: absolute;
  top: 100%;
  left: 50%;
  transform: translateX(-50%);
  margin-top: 4px;
  min-width: 300px;
  max-width: 420px;
  max-height: 280px;
  overflow-y: auto;
  background: var(--popover);
  border: 1px solid var(--border);
  border-radius: var(--radius);
  box-shadow: var(--shadow-paper-lifted);
  z-index: 50;
  padding: 4px;
}
.dd-item {
  padding: 6px 8px;
  border-radius: 3px;
  font-size: 11px;
}
.dd-item + .dd-item {
  margin-top: 2px;
}
.dd-accent { border-left: 2px solid var(--accent); }
.dd-destructive { border-left: 2px solid var(--destructive); }

/* transitions */
.notif-fade-enter-active,
.notif-fade-leave-active {
  transition: opacity 150ms ease, transform 150ms ease;
}
.notif-fade-enter-from { opacity: 0; transform: translateY(-4px); }
.notif-fade-leave-to { opacity: 0; transform: translateY(-4px); }

.dd-fade-enter-active,
.dd-fade-leave-active {
  transition: opacity 120ms ease, transform 120ms ease;
}
.dd-fade-enter-from { opacity: 0; transform: translateY(-4px); }
.dd-fade-leave-to { opacity: 0; transform: translateY(-4px); }

@media (prefers-reduced-motion: reduce) {
  .notif-fade-enter-active, .notif-fade-leave-active,
  .dd-fade-enter-active, .dd-fade-leave-active {
    transition: none !important;
  }
}
</style>
