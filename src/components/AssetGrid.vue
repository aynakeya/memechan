<script setup lang="ts">
import { ref } from 'vue'
import type { ContextMenuItem } from '@nuxt/ui'
import UBadge from '@nuxt/ui/components/Badge.vue'
import UButton from '@nuxt/ui/components/Button.vue'
import UContextMenu from '@nuxt/ui/components/ContextMenu.vue'
import UDropdownMenu from '@nuxt/ui/components/DropdownMenu.vue'
import UIcon from '@nuxt/ui/components/Icon.vue'
import { fileUrl } from '../api'
import type { Asset } from '../types'
defineProps<{
  assets: Asset[]
  selectedId: string | null
  copying: string | null
  compact: boolean
  importing: boolean
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
</script>
<template>
  <section class="asset-grid" :class="{ compact }" aria-label="表情网格">
    <UContextMenu
      v-for="asset in assets"
      :key="asset.id"
      :items="menuItems(asset)"
      :ui="{ content: 'w-56' }"
      @update:open="emit('menuOpen', $event)"
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
        <p v-if="asset.description || asset.ocrText" class="truncate px-1 text-xs text-muted">
          {{ asset.description || asset.ocrText }}
        </p>
      </article>
    </UContextMenu>
    <UButton
      class="add-tile"
      color="neutral"
      variant="outline"
      :loading="importing"
      @click="emit('import')"
    >
      <UIcon name="i-lucide-plus" class="size-6" />
      添加表情
    </UButton>
  </section>
</template>
