<script setup lang="ts">
import { computed, ref } from 'vue'
import type { ContextMenuItem } from '@nuxt/ui'
import UBadge from '@nuxt/ui/components/Badge.vue'
import UButton from '@nuxt/ui/components/Button.vue'
import UCheckbox from '@nuxt/ui/components/Checkbox.vue'
import UContextMenu from '@nuxt/ui/components/ContextMenu.vue'
import UDropdownMenu from '@nuxt/ui/components/DropdownMenu.vue'
import { fileUrl } from '../api'
import type { Asset } from '../types'

const props = defineProps<{
  asset: Asset
  selected: boolean
  active: boolean
  multiSelect: boolean
  copying: boolean
  menuItems: (asset: Asset) => ContextMenuItem[][]
}>()
const emit = defineEmits<{
  select: [asset: Asset, event?: MouseEvent, toggle?: boolean]
  copy: [asset: Asset]
  preview: [asset: Asset]
  favorite: [asset: Asset]
  menuOpen: [open: boolean]
}>()
const hovered = ref(false)
const items = computed(() => props.menuItems(props.asset))
const source = computed(() =>
  fileUrl(props.asset.animated && hovered.value ? props.asset.path : props.asset.thumbnail),
)
function focusAsset(event: FocusEvent) {
  if (
    !props.multiSelect &&
    !props.selected &&
    (event.target as HTMLElement).matches(':focus-visible')
  )
    emit('select', props.asset)
}
</script>
<template>
  <UContextMenu :items="items" :ui="{ content: 'w-56' }" @update:open="emit('menuOpen', $event)">
    <article
      class="asset-card"
      :class="{ selected, active }"
      tabindex="0"
      :aria-label="asset.name"
      @click="emit('select', asset, $event)"
      @dblclick="
        !multiSelect &&
        !$event.ctrlKey &&
        !$event.metaKey &&
        !$event.shiftKey &&
        emit('copy', asset)
      "
      @keydown.enter.prevent.stop="emit('copy', asset)"
      @keydown.space.prevent.stop="emit('preview', asset)"
      @focus="focusAsset($event)"
      @mouseenter="hovered = asset.animated"
      @mouseleave="hovered = false"
    >
      <div class="asset-image" :class="{ animated: asset.animated }">
        <div
          v-if="multiSelect"
          class="card-select"
          @click.stop="emit('select', asset, $event, true)"
          @dblclick.stop
          @keydown.stop
        >
          <UCheckbox :model-value="selected" :aria-label="`选择 ${asset.name}`" />
        </div>
        <img :src="source" :alt="asset.name" loading="lazy" draggable="false" />
        <UBadge
          v-if="asset.animated"
          class="absolute bottom-2 left-2"
          color="neutral"
          variant="solid"
          size="xs"
        >
          {{ hovered ? '播放中' : '动图' }}
        </UBadge>
        <UButton
          class="favorite-button"
          :color="asset.favorite ? 'primary' : 'neutral'"
          variant="soft"
          size="xs"
          icon="i-lucide-heart"
          :aria-label="asset.favorite ? '取消收藏' : '收藏表情'"
          :aria-pressed="asset.favorite"
          :ui="{ leadingIcon: asset.favorite ? 'fill-current' : '' }"
          @click.stop="emit('favorite', asset)"
          @dblclick.stop
          @keydown.stop
        />
        <UButton
          class="card-copy"
          size="xs"
          icon="i-lucide-copy"
          :loading="copying"
          @click.stop="emit('copy', asset)"
          @dblclick.stop
          @keydown.stop
        >
          {{ copying ? '处理中' : '复制' }}
        </UButton>
      </div>
      <div class="asset-caption">
        <h3 class="min-w-0 flex-1 truncate text-sm font-medium">{{ asset.name }}</h3>
        <UDropdownMenu
          :items="items"
          :ui="{ content: 'w-56' }"
          @update:open="emit('menuOpen', $event)"
        >
          <UButton
            color="neutral"
            variant="ghost"
            size="xs"
            icon="i-lucide-ellipsis"
            aria-label="更多操作"
            @click.stop
            @dblclick.stop
            @keydown.stop
          />
        </UDropdownMenu>
      </div>
      <p class="asset-description truncate px-1 text-xs text-muted">
        {{ asset.description || asset.ocrText }}
      </p>
    </article>
  </UContextMenu>
</template>
