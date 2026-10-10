<script setup lang="ts">
import UBadge from '@nuxt/ui/components/Badge.vue'
import UButton from '@nuxt/ui/components/Button.vue'
import UCheckbox from '@nuxt/ui/components/Checkbox.vue'
import UFormField from '@nuxt/ui/components/FormField.vue'
import UIcon from '@nuxt/ui/components/Icon.vue'
import UTextarea from '@nuxt/ui/components/Textarea.vue'
import { fileUrl } from '../api'
import { bytesLabel, sizeLabel, stateLabel } from '../format'
import type { Asset, Group, ModelStatus } from '../types'
defineProps<{
  asset: Asset
  groups: Group[]
  model: ModelStatus | null
  visionActive: boolean
  copySize: number
  copying: string | null
  saving: boolean
  descriptionSaved: string
}>()
const description = defineModel<string>({ required: true })
const emit = defineEmits<{
  close: []
  preview: []
  copy: []
  favorite: []
  save: []
  index: []
  ocr: []
  clear: [kind: 'ocr' | 'vision']
  describe: []
  settings: [tab: string]
  createGroup: []
  toggleGroup: [id: number]
  remove: []
}>()
</script>
<template>
  <aside class="details-panel" aria-label="表情详情">
    <div class="flex items-center justify-between border-b border-default p-4">
      <h2 class="text-sm font-semibold">表情详情</h2>
      <UButton
        color="neutral"
        variant="ghost"
        size="xs"
        icon="i-lucide-x"
        aria-label="关闭详情"
        @click="emit('close')"
      />
    </div>
    <div class="space-y-5 p-4">
      <button class="detail-preview" aria-label="放大预览" @click="emit('preview')">
        <img :src="fileUrl(asset.path)" :alt="asset.name" />
      </button>
      <div>
        <div class="flex items-center gap-2">
          <h3 class="min-w-0 flex-1 truncate font-semibold">{{ asset.name }}</h3>
          <UButton
            :color="asset.favorite ? 'primary' : 'neutral'"
            variant="ghost"
            size="xs"
            icon="i-lucide-heart"
            aria-label="切换收藏"
            :aria-pressed="asset.favorite"
            :ui="{ leadingIcon: asset.favorite ? 'fill-current' : '' }"
            @click="emit('favorite')"
          />
        </div>
        <p class="mt-1 text-xs text-muted">
          {{ asset.format.toUpperCase() }} · {{ asset.width }} × {{ asset.height }} ·
          {{ bytesLabel(asset.bytes) }}
        </p>
      </div>
      <UButton
        block
        icon="i-lucide-copy"
        :loading="copying === asset.id"
        :disabled="!!copying"
        @click="emit('copy')"
      >
        复制表情
        <span class="ml-auto text-xs opacity-80">{{ sizeLabel(copySize) }}</span>
      </UButton>
      <UFormField label="我的描述" name="description">
        <UTextarea
          v-model="description"
          class="w-full"
          :maxlength="4000"
          :rows="4"
          placeholder="添加描述以便搜索"
          @keydown.ctrl.enter="emit('save')"
        />
        <div class="mt-2 flex items-center justify-between">
          <span class="text-xs text-muted">{{ description.length }} / 4000</span>
          <UButton
            variant="link"
            size="xs"
            :loading="saving"
            :disabled="description === descriptionSaved"
            @click="emit('save')"
          >
            {{ saving ? '保存中…' : description === descriptionSaved ? '已保存' : '保存描述' }}
          </UButton>
        </div>
      </UFormField>
      <div class="space-y-3 border-t border-default pt-4">
        <div class="flex items-center justify-between gap-2">
          <h3 class="flex items-center gap-2 text-sm font-medium">
            <UIcon name="i-lucide-scan-text" class="size-4 text-muted" />
            图片中的文字
          </h3>
          <UButton
            size="xs"
            color="neutral"
            variant="ghost"
            icon="i-lucide-eraser"
            aria-label="清除这张图片的 OCR 文字"
            :disabled="!asset.ocrText && asset.ocrState === 'idle'"
            @click="emit('clear', 'ocr')"
          />
          <UBadge
            :color="asset.ocrState === 'error' ? 'error' : 'neutral'"
            variant="subtle"
            size="xs"
          >
            {{ stateLabel(asset.ocrState) }}
          </UBadge>
        </div>
        <p class="whitespace-pre-wrap text-sm text-muted">
          {{
            asset.ocrText ||
            (asset.ocrState === 'ready'
              ? '未识别到文字，可添加自定义描述。'
              : asset.ocrState === 'idle'
                ? '文字已清除，可重新识别。'
                : '启用本地 OCR 并导入模型后可识别。')
          }}
        </p>
        <UButton
          variant="link"
          size="xs"
          trailing-icon="i-lucide-arrow-right"
          :disabled="model?.ocr.running"
          @click="model?.ocrReady && model.ocrEnabled ? emit('ocr') : emit('settings', 'ocr')"
        >
          {{ model?.ocrReady && model.ocrEnabled ? '重新识别文字' : '设置本地 OCR' }}
        </UButton>
      </div>
      <div class="space-y-3 border-t border-default pt-4">
        <div class="flex items-center justify-between gap-2">
          <h3 class="text-sm font-medium">图片描述</h3>
          <UButton
            size="xs"
            color="neutral"
            variant="ghost"
            icon="i-lucide-eraser"
            aria-label="清除这张图片的描述"
            :disabled="!asset.aiDescription && asset.aiState === 'idle'"
            @click="emit('clear', 'vision')"
          />
          <UBadge
            :color="asset.aiState === 'error' ? 'error' : 'neutral'"
            variant="subtle"
            size="xs"
          >
            {{ stateLabel(asset.aiState) }}
          </UBadge>
        </div>
        <p v-if="asset.aiDescription" class="whitespace-pre-wrap text-sm text-muted">
          {{ asset.aiDescription }}
        </p>
        <p v-if="asset.aiModel" class="text-xs break-all text-dimmed">
          {{ asset.aiModel }}
        </p>
        <p v-if="asset.aiError" role="alert" class="text-xs text-error">
          {{ asset.aiError }}
        </p>
        <UButton
          variant="link"
          size="xs"
          icon="i-lucide-sparkles"
          :disabled="model?.vision.running"
          @click="visionActive ? emit('describe') : emit('settings', 'vision')"
        >
          {{
            !visionActive
              ? '配置图片理解 API'
              : asset.aiDescription
                ? '通过 API 重新生成'
                : '通过 API 生成描述'
          }}
        </UButton>
      </div>
      <div
        class="flex items-center justify-between border-t border-default pt-4 text-xs text-muted"
      >
        <span>索引：{{ stateLabel(asset.indexState) }}</span>
        <UButton
          variant="link"
          size="xs"
          :disabled="model?.index.running"
          @click="model?.ready ? emit('index') : emit('settings', 'models')"
        >
          重新索引
        </UButton>
      </div>
      <div class="space-y-3 border-t border-default pt-4">
        <div class="flex items-center justify-between">
          <h3 class="text-sm font-medium">所属分组</h3>
          <UButton variant="link" size="xs" @click="emit('createGroup')">新建</UButton>
        </div>
        <p v-if="!groups.length" class="text-sm text-muted">暂无分组</p>
        <UCheckbox
          v-for="group in groups"
          :key="group.id"
          :label="group.name"
          :model-value="asset.groups.includes(group.id)"
          @update:model-value="emit('toggleGroup', group.id)"
        />
      </div>
      <UButton block color="error" variant="ghost" icon="i-lucide-trash-2" @click="emit('remove')">
        从资料库删除
      </UButton>
    </div>
  </aside>
</template>
