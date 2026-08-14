<script setup lang="ts">
import type { ArtifactCandidate } from '@/features/artifact-preview/detectArtifacts'
import ArtifactPreviewCard from './ArtifactPreviewCard.vue'

defineProps<{
  candidates: ArtifactCandidate[]
  root: string
}>()
</script>

<template>
  <div v-if="root && candidates.length" class="artifact-preview-list" :aria-label="$t('artifactPreview.groupLabel')">
    <ArtifactPreviewCard
      v-for="candidate in candidates"
      :key="candidate.path"
      :candidate="candidate"
      :root="root"
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
