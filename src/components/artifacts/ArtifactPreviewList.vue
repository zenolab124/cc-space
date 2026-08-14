<script setup lang="ts">
import type { ArtifactCandidate } from '@/features/artifact-preview/detectArtifacts'
import ArtifactPreviewCard from './ArtifactPreviewCard.vue'

defineProps<{
  candidates: ArtifactCandidate[]
  root: string
  autoOpen?: boolean
}>()
</script>

<template>
  <div v-if="root && candidates.length" class="artifact-preview-list" :aria-label="$t('artifactPreview.groupLabel')">
    <ArtifactPreviewCard
      v-for="(candidate, index) in candidates"
      :key="candidate.path"
      :candidate="candidate"
      :root="root"
      :auto-open="autoOpen && index === 0"
    />
  </div>
</template>

<style scoped>
.artifact-preview-list {
  display: flex;
  min-width: 0;
  flex-direction: column;
  gap: 6px;
  margin-top: 8px;
}
</style>
