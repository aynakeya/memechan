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
import UButton from '@nuxt/ui/components/Button.vue'
import UIcon from '@nuxt/ui/components/Icon.vue'
import type { Asset } from '../types'
import AssetCard from './AssetCard.vue'
const props = defineProps<{
  assets: Asset[]
  selectedId: string | null
  selectedIds: Set<string>
  multiSelect: boolean
  copying: string | null
  compact: boolean
  importing: boolean
  scrollElement: HTMLElement | null
  menuItems: (asset: Asset) => ContextMenuItem[][]
}>()
const emit = defineEmits<{
  select: [asset: Asset, event?: MouseEvent, toggle?: boolean]
  copy: [asset: Asset]
  preview: [asset: Asset]
  favorite: [asset: Asset]
  import: []
  menuOpen: [open: boolean]
}>()
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
// Keep cards as siblings keyed by asset ID so changing columns never reparents them.
const cells = computed(() =>
  rows.value.flatMap((row) => {
    const start = row.index * columns.value
    const end = Math.min(start + columns.value, props.assets.length + 1)
    return Array.from({ length: end - start }, (_, column) => ({
      asset: props.assets[start + column],
      column,
      top: row.start - scrollMargin.value,
    }))
  }),
)
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
let measureFrame = 0
function scheduleMeasure() {
  if (measureFrame) return
  measureFrame = requestAnimationFrame(() => {
    measureFrame = 0
    measure()
  })
}
// Apply real resizes before paint so scrolling uses the new row count immediately.
const observer = new ResizeObserver(measure)
onMounted(() => {
  observer.observe(grid.value!)
  measure()
})
// Sibling toolbars can change the grid's scroll offset without resizing its width.
onUpdated(scheduleMeasure)
onBeforeUnmount(() => {
  observer.disconnect()
  cancelAnimationFrame(measureFrame)
})
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
    :style="{ height: `${height + 24}px`, '--asset-columns': columns }"
    aria-label="表情网格"
  >
    <div
      v-for="cell in cells"
      :key="cell.asset?.id ?? 'add'"
      class="asset-cell"
      :style="{
        '--asset-column': cell.column,
        transform: `translateY(${cell.top}px)`,
      }"
    >
      <AssetCard
        v-if="cell.asset"
        :asset="cell.asset"
        :selected="selectedIds.has(cell.asset.id)"
        :active="selectedId === cell.asset.id"
        :multi-select="multiSelect"
        :copying="copying === cell.asset.id"
        :menu-items="menuItems"
        @select="(asset, event, toggle) => emit('select', asset, event, toggle)"
        @copy="emit('copy', $event)"
        @preview="emit('preview', $event)"
        @favorite="emit('favorite', $event)"
        @menu-open="menuChanged($event, cell.asset!.id)"
      />
      <UButton
        v-else
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
