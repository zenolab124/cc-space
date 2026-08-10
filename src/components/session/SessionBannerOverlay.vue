<script setup lang="ts">
import SessionBanner from '@/components/SessionBanner.vue'

interface HookEvent {
  subtype: 'hook_started' | 'hook_response'
  hook_name: string
  hook_event: string
  output?: string
  exit_code?: number
}

defineProps<{
  visible: boolean
  sessionId: string
  resumed: boolean
  cwd: string
  model: string | null
  effort: string | null
  features: string[]
  hookEvents: HookEvent[]
}>()
</script>

<template>
  <Transition name="banner-float">
    <div
      v-if="visible && sessionId"
      class="absolute top-2 left-4 right-4 z-30 pointer-events-none"
    >
      <div class="pointer-events-auto shadow-paper-lifted rounded-md bg-popover/40 backdrop-blur-md border border-border">
        <SessionBanner
          :session-id="sessionId"
          :resumed="resumed"
          :cwd="cwd"
          :model="model"
          :effort="effort"
          :features="features"
          :hook-events="hookEvents"
        />
      </div>
    </div>
  </Transition>
</template>

<style scoped>
.banner-float-enter-active,
.banner-float-leave-active {
  transition: opacity 200ms ease, transform 200ms ease;
}
.banner-float-enter-from,
.banner-float-leave-to {
  opacity: 0;
  transform: translateY(-6px);
}
</style>
