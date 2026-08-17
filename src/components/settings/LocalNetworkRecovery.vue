<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { useI18n } from 'vue-i18n'
import { useConfirm } from '@/composables/useConfirm'
import { useNotifications } from '@/composables/useNotifications'

type Status = 'granted' | 'denied' | 'undetermined' | 'targetNotRunning' | 'unverified' | 'unknown'
interface AppCopy { path: string; current: boolean }

const props = defineProps<{ status: Status }>()
const emit = defineEmits<{ request: [] }>()
const { t } = useI18n()
const { confirm } = useConfirm()
const { notifyTransient } = useNotifications()
const copies = ref<AppCopy[]>([])
const loading = ref(false)
const movingPath = ref<string | null>(null)
const showRecovery = computed(() => props.status === 'denied' || copies.value.length > 1)

async function loadCopies() {
  loading.value = true
  try {
    copies.value = await invoke<AppCopy[]>('list_app_copies')
  } catch {
    copies.value = []
  } finally {
    loading.value = false
  }
}

async function reveal(path: string) {
  try {
    await invoke('reveal_in_finder', { path })
  } catch (cause) {
    notifyTransient(t('settings.permCheck.recoveryRevealFailed'), String(cause))
  }
}

async function moveToTrash(copy: AppCopy) {
  const approved = await confirm(
    t('settings.permCheck.recoveryRemoveConfirm', { path: copy.path }),
    t('settings.permCheck.recoveryMoveToTrash'),
  )
  if (!approved) return
  movingPath.value = copy.path
  try {
    await invoke<string>('move_app_copy_to_trash', { path: copy.path })
    notifyTransient(t('settings.permCheck.recoveryRemoved'))
    await loadCopies()
  } catch (cause) {
    const code = String(cause)
    const detail = code.includes('APP_COPY_STALE')
      ? t('settings.permCheck.recoveryStale')
      : code.includes('APP_COPY_CURRENT')
        ? t('settings.permCheck.recoveryCurrentBlocked')
        : code.includes('TRASH_UNAVAILABLE') || code.includes('APP_COPY_MOVE_FAILED')
          ? t('settings.permCheck.recoveryManualRemove')
          : t('settings.permCheck.recoveryInvalid')
    notifyTransient(t('settings.permCheck.recoveryRemoveFailed'), detail)
  } finally {
    movingPath.value = null
  }
}

onMounted(loadCopies)
</script>

<template>
  <section v-if="showRecovery" class="local-network-recovery">
    <div class="local-network-recovery__header">
      <span class="local-network-recovery__icon"><span class="i-carbon-tool-box" /></span>
      <div class="min-w-0 flex-1">
        <div class="text-xs font-semibold">{{ t('settings.permCheck.recoveryTitle') }}</div>
        <p class="local-network-recovery__description">
          {{ t('settings.permCheck.recoveryDesc') }}
        </p>
      </div>
      <button type="button" class="perm-btn" :disabled="loading" @click="loadCopies">
        <span :class="loading ? 'i-carbon-circle-dash animate-spin' : 'i-carbon-renew'" class="h-3 w-3" />
        {{ t('settings.permCheck.recoveryScan') }}
      </button>
    </div>

    <div v-if="copies.length > 1" class="local-network-recovery__copies">
      <div class="local-network-recovery__warning">
        <span class="i-carbon-warning-alt h-4 w-4 shrink-0" />
        {{ t('settings.permCheck.recoveryDuplicates', { count: copies.length }) }}
      </div>
      <div v-for="copy in copies" :key="copy.path" class="local-network-recovery__copy">
        <span class="i-carbon-application h-4 w-4 shrink-0 opacity-70" />
        <div class="min-w-0 flex-1">
          <div class="truncate text-[11px]" :title="copy.path">{{ copy.path }}</div>
          <div v-if="copy.current" class="text-[10px] text-primary">
            {{ t('settings.permCheck.recoveryCurrent') }}
          </div>
        </div>
        <button type="button" class="perm-btn" @click="reveal(copy.path)">
          {{ t('settings.permCheck.recoveryReveal') }}
        </button>
        <button
          v-if="!copy.current"
          type="button"
          class="perm-btn perm-btn--danger"
          :disabled="movingPath !== null"
          @click="moveToTrash(copy)"
        >
          <span v-if="movingPath === copy.path" class="i-carbon-circle-dash h-3 w-3 animate-spin" />
          {{ t('settings.permCheck.recoveryMoveToTrash') }}
        </button>
      </div>
    </div>

    <div class="local-network-recovery__actions">
      <button type="button" class="perm-btn" @click="emit('request')">
        <span class="i-carbon-network-3 h-3 w-3" />
        {{ t('settings.permCheck.testAndRequest') }}
      </button>
    </div>
  </section>
</template>

<style scoped>
.local-network-recovery {
  overflow: hidden;
  border: 1px solid color-mix(in srgb, var(--accent) 45%, var(--border));
  border-radius: var(--radius);
  background: color-mix(in srgb, var(--accent) 4%, var(--card));
  box-shadow: var(--shadow-paper);
}
.local-network-recovery__header,
.local-network-recovery__actions,
.local-network-recovery__copy,
.local-network-recovery__warning {
  display: flex;
  align-items: center;
  gap: 10px;
}
.local-network-recovery__header { align-items: flex-start; padding: 14px 16px; }
.local-network-recovery__icon {
  display: grid;
  width: 27px;
  height: 27px;
  flex-shrink: 0;
  place-items: center;
  border: 1px solid color-mix(in srgb, var(--accent) 45%, var(--border));
  border-radius: var(--radius);
  color: var(--accent);
  background: color-mix(in srgb, var(--accent) 9%, transparent);
}
.local-network-recovery__description {
  margin: 3px 0 0;
  color: var(--muted-foreground);
  font-size: 11px;
  line-height: 1.55;
}
.local-network-recovery__copies { border-top: 1px solid var(--border); }
.local-network-recovery__warning {
  padding: 9px 16px;
  color: var(--accent);
  background: color-mix(in srgb, var(--accent) 8%, transparent);
  font-size: 11px;
}
.local-network-recovery__copy { min-height: 44px; padding: 8px 16px; border-top: 1px solid var(--border); }
.local-network-recovery__actions { justify-content: flex-end; padding: 10px 16px; border-top: 1px solid var(--border); }
.perm-btn {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  padding: 2px 8px;
  border: 1px solid var(--border);
  border-radius: 5px;
  color: var(--foreground);
  background: var(--card);
  font-size: 11px;
  white-space: nowrap;
}
.perm-btn:hover:not(:disabled) { background: var(--muted); }
.perm-btn:disabled { opacity: 0.5; }
.perm-btn--danger { color: var(--destructive); }
@media (max-width: 680px) {
  .local-network-recovery__header { flex-wrap: wrap; }
  .local-network-recovery__copy { align-items: flex-start; flex-wrap: wrap; }
}
</style>
