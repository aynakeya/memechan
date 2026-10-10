<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import UAlert from '@nuxt/ui/components/Alert.vue'
import UBadge from '@nuxt/ui/components/Badge.vue'
import UButton from '@nuxt/ui/components/Button.vue'
import UFormField from '@nuxt/ui/components/FormField.vue'
import UIcon from '@nuxt/ui/components/Icon.vue'
import UInput from '@nuxt/ui/components/Input.vue'
import UModal from '@nuxt/ui/components/Modal.vue'
import UProgress from '@nuxt/ui/components/Progress.vue'
import USelect from '@nuxt/ui/components/Select.vue'
import USwitch from '@nuxt/ui/components/Switch.vue'
import UTabs from '@nuxt/ui/components/Tabs.vue'
import { api } from '../api'
import { errorMessage, useNotifications } from '../composables/useNotifications'
import type { ModelStatus, Settings, VisionConfig, VisionStatus } from '../types'
const props = defineProps<{ settings: Settings; model: ModelStatus | null }>()
const open = defineModel<boolean>('open', { required: true })
const settingsTab = defineModel<string>('tab', { required: true })
const vision = defineModel<VisionStatus | null>('vision', { required: true })
const emit = defineEmits<{ saved: [settings: Settings]; refresh: [] }>()
const draft = ref({ ...props.settings })
const settingsSaving = ref(false)
const importingModels = ref(false)
const notify = useNotifications()
const visionConfig = ref<VisionConfig>({ baseUrl: '', model: '', autoDescribe: false, apiKey: '' })
const showVisionKey = ref(false),
  visionSaving = ref(false),
  visionTesting = ref(false)
const visionTestResult = ref(''),
  visionError = ref('')
const visionDirty = computed(
  () =>
    !!vision.value &&
    (visionConfig.value.baseUrl !== vision.value.config.baseUrl ||
      visionConfig.value.model !== vision.value.config.model ||
      visionConfig.value.autoDescribe !== vision.value.config.autoDescribe ||
      visionConfig.value.apiKey !== vision.value.config.apiKey),
)

const requiredMissing = computed(
  () =>
    props.model?.missing.filter((name) =>
      settingsTab.value === 'ocr' ? name.startsWith('ocr-') : !name.startsWith('ocr-'),
    ) ?? [],
)
const settingsTabs = [
  { label: '复制与资料库', value: 'general' },
  { label: 'OCR', value: 'ocr' },
  { label: 'Embedding', value: 'models' },
  { label: '图片理解 API', value: 'vision' },
]
const embeddingModels = [
  { label: 'Qwen3-Embedding-0.6B', value: 'qwen3-0.6b' },
  { label: 'Qwen3-Embedding-4B', value: 'qwen3-4b' },
  { label: 'Qwen3-Embedding-8B', value: 'qwen3-8b' },
]

watch(open, (opened) => {
  showVisionKey.value = false
  if (opened) {
    draft.value = { ...props.settings }
    if (vision.value) visionConfig.value = { ...vision.value.config }
  }
})
async function saveSettings() {
  if (settingsSaving.value) return
  settingsSaving.value = true
  const value = { ...draft.value }
  try {
    await api.saveSettings(value)
    emit('saved', value)
    emit('refresh')
    notify('设置已保存')
  } catch (error) {
    draft.value = { ...props.settings }
    notify(errorMessage(error), true)
  } finally {
    settingsSaving.value = false
  }
}
async function importModels() {
  importingModels.value = true
  try {
    if (await api.importModels()) emit('refresh')
  } catch (error) {
    notify(errorMessage(error), true)
  } finally {
    importingModels.value = false
  }
}

async function pauseInstall() {
  try {
    await api.pauseInstall()
    emit('refresh')
  } catch (error) {
    notify(errorMessage(error), true)
  }
}

async function refreshVision() {
  vision.value = await api.visionStatus()
  visionConfig.value = { ...vision.value.config }
}
async function saveVision() {
  visionSaving.value = true
  visionError.value = ''
  visionTestResult.value = ''
  try {
    await api.saveVisionConfig({ ...visionConfig.value })
    await refreshVision()
    emit('refresh')
    notify('API 配置已保存')
  } catch (error) {
    visionError.value = errorMessage(error)
  } finally {
    visionSaving.value = false
  }
}
async function testVision() {
  visionTesting.value = true
  visionError.value = ''
  visionTestResult.value = ''
  try {
    const result = await api.testVision()
    visionTestResult.value = `${result.model}：${result.text}`
  } catch (error) {
    visionError.value = errorMessage(error)
  } finally {
    visionTesting.value = false
  }
}
</script>

<template>
  <UModal
    v-model:open="open"
    title="偏好设置"
    :close="{ 'aria-label': '关闭弹窗' }"
    :ui="{ content: 'max-w-xl' }"
  >
    <template #body>
      <UTabs
        v-model="settingsTab"
        :items="settingsTabs"
        :content="false"
        variant="link"
        class="mb-5"
      />
      <div v-if="settingsTab === 'general'" class="space-y-6">
        <UFormField
          label="默认复制尺寸"
          description="双击和普通复制时使用，按比例缩放。填 0 保留原尺寸。"
          orientation="horizontal"
          :ui="{ wrapper: 'flex-1', container: 'shrink-0' }"
        >
          <form class="flex items-center gap-2" @submit.prevent="saveSettings()">
            <UInput
              v-model.number="draft.copySize"
              type="number"
              :min="0"
              :max="4096"
              required
              aria-label="默认复制最长边"
              class="w-24"
            >
              <template #trailing><span class="text-xs text-muted">px</span></template>
            </UInput>
            <UButton type="submit" variant="soft" :loading="settingsSaving">保存</UButton>
          </form>
        </UFormField>
        <UFormField
          label="允许放大小图"
          description="关闭时，小于目标尺寸的图片保留原尺寸。"
          orientation="horizontal"
        >
          <USwitch
            v-model="draft.allowUpscale"
            :disabled="settingsSaving"
            @update:model-value="saveSettings()"
          />
        </UFormField>
        <UAlert
          color="neutral"
          variant="subtle"
          icon="i-lucide-info"
          description="原图始终保留。GIF 缩放保留动画；动态 WebP / APNG 缩放后以 GIF 复制，颜色和透明效果可能变化。原尺寸复制保留原文件格式。"
        />
      </div>
      <div v-else-if="settingsTab === 'models' || settingsTab === 'ocr'" class="space-y-4">
        <UFormField
          v-if="settingsTab === 'ocr'"
          label="启用本地 OCR"
          description="关闭后仍可用 API 图片描述和自定义描述建立索引。"
          orientation="horizontal"
        >
          <USwitch
            v-model="draft.ocrEnabled"
            aria-label="启用本地 OCR"
            :disabled="settingsSaving"
            @update:model-value="saveSettings()"
          />
        </UFormField>
        <UFormField
          v-else
          label="自动建立搜索索引"
          description="模型就绪后，导入图片和修改描述会自动索引。"
          orientation="horizontal"
        >
          <USwitch
            v-model="draft.autoIndex"
            :disabled="settingsSaving"
            @update:model-value="saveSettings()"
          />
        </UFormField>
        <div v-if="settingsTab === 'ocr'" class="model-item">
          <UIcon name="i-lucide-scan-text" class="size-5 shrink-0 text-primary" />
          <div class="flex-1">
            <h3 class="text-sm font-medium">图片文字识别</h3>
            <p class="mt-1 text-xs text-muted">PP-OCRv5 mobile · 中文 / 英文 / 日文</p>
          </div>
          <UBadge color="neutral" variant="subtle" size="xs">
            {{ model?.ocrReady ? '已安装' : '待安装' }}
          </UBadge>
        </div>
        <template v-else>
          <UFormField
            label="向量模型"
            description="切换会重建文字索引，保留原图、OCR 和描述。请先暂停本地任务。"
          >
            <div class="flex items-center gap-3">
              <USelect
                v-model="draft.embeddingModel"
                :items="embeddingModels"
                aria-label="向量模型"
                class="flex-1"
                :disabled="
                  settingsSaving || model?.job.running || model?.ocr.running || model?.index.running
                "
                @update:model-value="saveSettings()"
              />
              <UBadge color="neutral" variant="subtle" size="xs">
                {{ model?.ready ? '已安装' : '待安装' }}
              </UBadge>
            </div>
          </UFormField>
          <p class="text-xs text-muted">
            {{
              draft.embeddingModel === 'qwen3-4b'
                ? '2560 维 · 建议 INT8 · CPU 推理'
                : draft.embeddingModel === 'qwen3-8b'
                  ? '4096 维 · 建议 INT8 · 内存占用和推理时间高于 4B'
                  : '1024 维 · CPU 推理'
            }}
          </p>
        </template>
        <div v-if="model?.job.running" class="space-y-3">
          <div class="flex justify-between gap-3 text-xs text-muted">
            <span>{{ model.job.message }}</span>
            <span>{{ model.job.done }} / {{ model.job.total }}</span>
          </div>
          <UProgress
            :model-value="model.job.done"
            :max="Math.max(1, model.job.total)"
            aria-label="模型处理进度"
          />
          <UButton variant="soft" size="sm" icon="i-lucide-pause" @click="pauseInstall()">
            暂停任务
          </UButton>
        </div>
        <UAlert
          v-if="model?.job.error"
          role="alert"
          color="error"
          variant="subtle"
          :description="model.job.error"
        />
        <p
          v-else-if="model?.job.message && !model.job.running"
          class="text-xs text-muted"
          role="status"
        >
          {{ model.job.message }}
        </p>
        <UButton
          :variant="requiredMissing.length ? 'solid' : 'outline'"
          icon="i-lucide-folder-open"
          :loading="importingModels"
          :disabled="model?.job.running"
          @click="importModels()"
        >
          {{ importingModels ? '选择模型文件…' : '导入本地模型' }}
        </UButton>
        <p class="text-xs text-muted">
          {{
            settingsTab === 'ocr'
              ? '可多选 OCR 文件。'
              : '选择 memechan-model.json，关联权重与分词器会一并导入。'
          }}
          导入与推理均不联网，原文件保留。
        </p>
        <p v-if="requiredMissing.length" class="text-xs break-words text-muted">
          还需导入：{{ requiredMissing.join('、') }}
        </p>
        <details class="model-location">
          <summary>模型保存位置与离线安装</summary>
          <code>{{ model?.directory || '桌面应用数据目录 / models' }}</code>
          <p>
            支持 README 列出的{{
              settingsTab === 'ocr' ? ' PP-OCRv5 mobile 模型' : '所选 Qwen3-Embedding 的模型包'
            }}，无需重命名。
          </p>
        </details>
      </div>
      <div v-else class="space-y-4">
        <p class="text-xs text-muted">
          生成时会将压缩图片发送到所配置的 API，可能产生费用。文字索引在本地建立。
        </p>
        <form class="space-y-4" @submit.prevent="saveVision()">
          <UFormField label="API 地址" name="vision-url" required>
            <UInput
              v-model="visionConfig.baseUrl"
              class="w-full"
              type="url"
              placeholder="https://api.deepseek.com"
              required
              :disabled="model?.vision.running || visionTesting || visionSaving"
              @update:model-value="visionConfig.apiKey = ''"
            />
          </UFormField>
          <UFormField label="模型名称" name="vision-model" required>
            <UInput
              v-model="visionConfig.model"
              class="w-full"
              placeholder="deepseek-v4-flash"
              required
              :disabled="model?.vision.running || visionTesting || visionSaving"
            />
          </UFormField>
          <UFormField
            label="API Key"
            name="vision-key"
            description="保存在本机资料库，未加密。留空并保存可清除密钥。"
          >
            <UInput
              v-model="visionConfig.apiKey"
              class="w-full"
              :type="showVisionKey ? 'text' : 'password'"
              autocomplete="off"
              placeholder="输入 API Key，本机免密服务可留空"
              :disabled="model?.vision.running || visionTesting || visionSaving"
            >
              <template #trailing>
                <UButton
                  type="button"
                  color="neutral"
                  variant="link"
                  size="sm"
                  :icon="showVisionKey ? 'i-lucide-eye-off' : 'i-lucide-eye'"
                  :aria-label="showVisionKey ? '隐藏 API Key' : '显示 API Key'"
                  :aria-pressed="showVisionKey"
                  @click="showVisionKey = !showVisionKey"
                />
              </template>
            </UInput>
          </UFormField>
          <UFormField
            label="导入后自动生成描述"
            description="仅自动处理新导入的图片。已有图片可在任务管理中生成。"
            orientation="horizontal"
          >
            <USwitch
              v-model="visionConfig.autoDescribe"
              :disabled="model?.vision.running || visionTesting || visionSaving"
            />
          </UFormField>
          <div class="flex flex-wrap gap-2">
            <UButton
              type="submit"
              :loading="visionSaving"
              :disabled="model?.vision.running || visionTesting"
            >
              保存 API 配置
            </UButton>
            <UButton
              variant="outline"
              :loading="visionTesting"
              :disabled="!vision?.active || visionDirty || visionSaving || model?.vision.running"
              @click="testVision()"
            >
              测试图片理解
            </UButton>
          </div>
        </form>
        <p class="text-xs text-muted">测试使用内置色块图，不读取资料库。</p>
        <UAlert
          v-if="visionError"
          role="alert"
          color="error"
          variant="subtle"
          :description="visionError"
        />
        <UAlert
          v-if="visionTestResult"
          role="status"
          color="success"
          variant="subtle"
          :description="visionTestResult"
        />
      </div>
    </template>
  </UModal>
</template>
