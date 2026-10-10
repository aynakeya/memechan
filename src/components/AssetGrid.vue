<script setup lang="ts">
import {
  computed,
  nextTick,
  onBeforeUnmount,
  onMounted,
  onUpdated,
  ref,
  useTemplateRef,
  watch,
} from 'vue'
import { defaultRangeExtractor, useVirtualizer } from '@tanstack/vue-virtual'
import type { Range } from '@tanstack/vue-virtual'
import type { ContextMenuItem } from '@nuxt/ui'
import UBadge from '@nuxt/ui/components/Badge.vue'
import UButton from '@nuxt/ui/components/Button.vue'
import UContextMenu from '@nuxt/ui/components/ContextMenu.vue'
import UDropdownMenu from '@nuxt/ui/components/DropdownMenu.vue'
import UIcon from '@nuxt/ui/components/Icon.vue'
import { fileUrl } from '../api'
import type { Asset } from '../types'
const props = defineProps<{
  assets: Asset[]
  selectedId: string | null
  copying: string | null
  compact: boolean
  importing: boolean
  scrollElement: HTMLElement | null
  menuItems: (asset: Asset) => ContextMenuItem[][]
}>()
const emit = defineEmits<{
  select: [asset: Asset]
  copy: [asset: Asset]
  preview: [asset: Asset]
  favorite: [asset: Asset]
  import: []
  menuOpen: [open: boolean]
}>()
const hovered = ref<string | null>(null)
const grid = useTemplateRef<HTMLElement>('grid')
const columns = ref(1)
const rowHeight = ref(214)
const gap = ref(16)
const scrollMargin = ref(0)
const menuAsset = ref<string | null>(null)
const menuRow = computed(() => {
  const index = props.assets.findIndex((asset) => asset.id === menuAsset.value)
  return index < 0 ? -1 : Math.floor(index / columns.value)
})
const virtualizer = useVirtualizer(
  computed(() => {
    const openRow = menuRow.value
    return {
      count: Math.ceil((props.assets.length + 1) / columns.value),
      getScrollElement: () => props.scrollElement,
      estimateSize: () => rowHeight.value,
      gap: gap.value,
      scrollMargin: scrollMargin.value,
      overscan: 3,
      rangeExtractor: (range: Range) => {
        const rows = defaultRangeExtractor(range)
        // Keep the menu's trigger mounted while its portal is open.
        if (openRow >= 0 && !rows.includes(openRow)) rows.push(openRow)
        return rows.sort((a, b) => a - b)
      },
    }
  }),
)
const rows = computed(() => virtualizer.value.getVirtualItems())
const height = computed(() => virtualizer.value.getTotalSize())
function measure() {
  if (!grid.value || !props.scrollElement || !grid.value.clientWidth) return
  const style = getComputedStyle(grid.value)
  const width = grid.value.clientWidth
  gap.value = Number.parseFloat(style.getPropertyValue('--asset-gap'))
  const minWidth = Number.parseFloat(style.getPropertyValue('--asset-min-width'))
  columns.value = Math.max(1, Math.floor((width + gap.value) / (minWidth + gap.value)))
  rowHeight.value = (width - (columns.value - 1) * gap.value) / columns.value + 49
  scrollMargin.value =
    grid.value.getBoundingClientRect().top -
    props.scrollElement.getBoundingClientRect().top +
    props.scrollElement.scrollTop
}
const observer = new ResizeObserver(measure)
onMounted(() => {
  observer.observe(grid.value!)
  measure()
})
onUpdated(measure)
onBeforeUnmount(() => observer.disconnect())
watch(rowHeight, () => virtualizer.value.measure())
watch(
  () => props.compact,
  async () => {
    await nextTick()
    measure()
  },
)
function menuChanged(open: boolean, id: string) {
  menuAsset.value = open ? id : null
  emit('menuOpen', open)
}
</script>
<template>
  <section
    ref="grid"
    class="asset-grid"
    :class="{ compact }"
    :style="{ height: `${height + 24}px` }"
    aria-label="表情网格"
  >
    <div
      v-for="row in rows"
      :key="row.index"
      class="asset-row"
      :style="{
        gridTemplateColumns: `repeat(${columns}, minmax(0, 1fr))`,
        transform: `translateY(${row.start - scrollMargin}px)`,
      }"
    >
      <UContextMenu
        v-for="asset in assets.slice(row.index * columns, (row.index + 1) * columns)"
        :key="asset.id"
        :items="menuItems(asset)"
        :ui="{ content: 'w-56' }"
        @update:open="menuChanged($event, asset.id)"
      >
        <article
          class="asset-card"
          :class="{ selected: selectedId === asset.id }"
          tabindex="0"
          :aria-label="asset.name"
          @click="emit('select', asset)"
          @dblclick="emit('copy', asset)"
          @keydown.enter.prevent.stop="emit('copy', asset)"
          @keydown.space.prevent.stop="emit('preview', asset)"
          @focus="emit('select', asset)"
          @mouseenter="hovered = asset.id"
          @mouseleave="hovered = null"
        >
          <div class="asset-image" :class="{ animated: asset.animated }">
            <img
              :src="fileUrl(asset.animated && hovered === asset.id ? asset.path : asset.thumbnail)"
              :alt="asset.name"
              loading="lazy"
              draggable="false"
            />
            <UBadge
              v-if="asset.animated"
              class="absolute bottom-2 left-2"
              color="neutral"
              variant="solid"
              size="xs"
            >
              {{ hovered === asset.id ? '播放中' : '动图' }}
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
              :loading="copying === asset.id"
              @click.stop="emit('copy', asset)"
              @dblclick.stop
              @keydown.stop
            >
              {{ copying === asset.id ? '处理中' : '复制' }}
            </UButton>
          </div>
          <div class="asset-caption">
            <h3 class="min-w-0 flex-1 truncate text-sm font-medium">{{ asset.name }}</h3>
            <UDropdownMenu
              :items="menuItems(asset)"
              :ui="{ content: 'w-56' }"
              @update:open="menuChanged($event, asset.id)"
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
      <UButton
        v-if="row.index === Math.floor(assets.length / columns)"
        class="add-tile"
        color="neutral"
        variant="outline"
        :loading="importing"
        @click="emit('import')"
      >
        <UIcon name="i-lucide-plus" class="size-6" />
        添加表情
      </UButton>
    </div>
  </section>
</template>
