<script setup lang="ts">
import { computed, ref } from 'vue'
import UAlert from '@nuxt/ui/components/Alert.vue'
import UBadge from '@nuxt/ui/components/Badge.vue'
import UButton from '@nuxt/ui/components/Button.vue'
import UIcon from '@nuxt/ui/components/Icon.vue'
import UModal from '@nuxt/ui/components/Modal.vue'
import UProgress from '@nuxt/ui/components/Progress.vue'
import { api } from '../api'
import { errorMessage, useNotifications } from '../composables/useNotifications'
import type { ClearKind, ModelStatus, TaskAction, TaskKind } from '../types'

const props = defineProps<{
  model: ModelStatus | null
  visionActive: boolean
  assetCount: number
}>()
const open = defineModel<boolean>('open', { required: true })
const emit = defineEmits<{ refresh: []; settings: [tab: string]; clear: [kind: ClearKind] }>()
const busy = ref<TaskKind | null>(null)
const notify = useNotifications()
const tasks = computed(() =>
  props.model
    ? [
        {
          kind: 'ocr' as const,
          title: '本地 OCR',
          completedLabel: '已识别',
          incompleteLabel: '未识别',
          start: '开始识别',
          icon: 'i-lucide-scan-text',
          job: props.model.ocr,
          available: props.model.ocrReady && props.model.ocrEnabled,
          setup: 'ocr',
          reason: !props.model.ocrEnabled ? '已停用' : '未安装 OCR 模型',
          generate: '识别未处理图片',
          clear: '清除全部 OCR 文字',
        },
        {
          kind: 'vision' as const,
          title: '图片理解 API',
          completedLabel: '已生成',
          incompleteLabel: '未生成',
          start: '生成描述',
          icon: 'i-lucide-sparkles',
          job: props.model.vision,
          available: props.visionActive,
          setup: 'vision',
          reason: '未配置 API',
          generate: '生成缺失描述',
          clear: '清除全部图片描述',
        },
        {
          kind: 'index' as const,
          title: '文字向量索引',
          completedLabel: '已索引',
          incompleteLabel: '未索引',
          start: '建立索引',
          icon: 'i-lucide-text-search',
          job: props.model.index,
          available: props.model.ready && props.model.semanticEnabled,
          setup: 'models',
          reason: !props.model.semanticEnabled ? '已停用' : '未安装 Qwen 模型',
          generate: '建立缺失索引',
          clear: '清除全部文字索引',
        },
      ]
    : [],
)
async function control(kind: TaskKind, action: TaskAction) {
  busy.value = kind
  try {
    await api.task(kind, action)
    emit('refresh')
  } catch (error) {
    notify(errorMessage(error), true)
  } finally {
    busy.value = null
  }
}
</script>

<template>
  <UModal
    v-model:open="open"
    title="任务管理"
    :close="{ 'aria-label': '关闭任务管理' }"
    :ui="{ content: 'max-w-2xl' }"
  >
    <template #body>
      <div class="space-y-4">
        <section
          v-for="task in tasks"
          :key="task.kind"
          :aria-label="task.title"
          class="space-y-3 rounded-lg border border-default p-4"
        >
          <div class="flex items-center gap-2">
            <UIcon :name="task.icon" class="size-4 text-primary" />
            <h3 class="flex-1 text-sm font-medium">{{ task.title }}</h3>
            <UBadge
              :color="
                task.job.failed || task.job.error
                  ? 'error'
                  : task.job.running
                    ? 'primary'
                    : 'neutral'
              "
              variant="subtle"
            >
              {{
                !task.available
                  ? task.reason
                  : task.job.running
                    ? task.job.paused
                      ? '暂停中'
                      : '处理中'
                    : task.job.failed || task.job.error
                      ? '有失败项'
                      : !task.job.incomplete
                        ? task.job.completed
                          ? '已完成'
                          : '暂无任务'
                        : task.job.paused
                          ? '已暂停'
                          : task.job.pending
                            ? '待处理'
                            : task.incompleteLabel
              }}
            </UBadge>
          </div>
          <div class="flex flex-wrap gap-x-4 gap-y-1 text-xs text-muted">
            <span>{{ task.completedLabel }} {{ task.job.completed }}</span>
            <span>{{ task.incompleteLabel }} {{ task.job.incomplete }}</span>
            <span v-if="task.job.failed" class="text-error">其中失败 {{ task.job.failed }}</span>
            <span v-if="task.job.completed + task.job.incomplete" class="ml-auto tabular-nums">
              {{
                Math.round((task.job.completed / (task.job.completed + task.job.incomplete)) * 100)
              }}%
            </span>
          </div>
          <UProgress
            v-if="task.job.completed + task.job.incomplete"
            role="group"
            :model-value="task.job.completed"
            :max="task.job.completed + task.job.incomplete"
            :color="!task.available || task.job.paused ? 'neutral' : 'primary'"
            :aria-label="`${task.title}进度`"
          />
          <p
            v-if="task.job.running && task.job.message"
            role="status"
            class="truncate text-xs text-muted"
          >
            {{ task.job.message }}
          </p>
          <UAlert
            v-if="task.job.error"
            role="alert"
            color="error"
            variant="subtle"
            :description="task.job.error"
          />
          <div class="flex flex-wrap gap-2">
            <UButton
              v-if="!task.available"
              size="sm"
              variant="soft"
              @click="emit('settings', task.setup)"
            >
              前往设置
            </UButton>
            <UButton
              v-if="task.job.running"
              size="sm"
              variant="soft"
              icon="i-lucide-pause"
              :loading="busy === task.kind"
              :disabled="task.job.paused"
              @click="control(task.kind, 'pause')"
            >
              暂停
            </UButton>
            <template v-else-if="task.available">
              <UButton
                v-if="task.job.pending"
                size="sm"
                icon="i-lucide-play"
                :disabled="!!busy"
                @click="control(task.kind, 'resume')"
              >
                {{ task.job.paused ? '继续' : task.start }}
              </UButton>
              <UButton
                v-if="task.job.failed || task.job.error"
                size="sm"
                variant="outline"
                icon="i-lucide-rotate-cw"
                :disabled="!!busy"
                @click="control(task.kind, 'retry')"
              >
                重试失败项
              </UButton>
              <UButton
                v-if="task.job.incomplete > task.job.pending + task.job.failed"
                size="sm"
                variant="outline"
                :disabled="!assetCount || !!busy"
                @click="control(task.kind, 'generate')"
              >
                {{ task.generate }}
              </UButton>
            </template>
            <UButton
              size="sm"
              color="neutral"
              variant="outline"
              icon="i-lucide-eraser"
              class="ml-auto"
              :disabled="!assetCount"
              @click="emit('clear', task.kind)"
            >
              {{ task.clear }}
            </UButton>
          </div>
        </section>
        <p class="text-xs text-muted">
          暂停在当前图片处理完成后生效。清除后需手动重新生成或建立索引。
        </p>
        <p v-if="model?.job.running" role="status" class="text-xs text-muted">
          模型安装：{{ model.job.message }}
        </p>
        <p v-if="model?.import.running" role="status" class="text-xs text-muted">
          图片导入：{{ model.import.done }} / {{ model.import.total }}
        </p>
      </div>
    </template>
  </UModal>
</template>
