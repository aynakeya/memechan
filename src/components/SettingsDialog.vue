<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import UAlert from '@nuxt/ui/components/Alert.vue'
import UBadge from '@nuxt/ui/components/Badge.vue'
import UButton from '@nuxt/ui/components/Button.vue'
import UCheckbox from '@nuxt/ui/components/Checkbox.vue'
import UFormField from '@nuxt/ui/components/FormField.vue'
import UIcon from '@nuxt/ui/components/Icon.vue'
import UInput from '@nuxt/ui/components/Input.vue'
import UModal from '@nuxt/ui/components/Modal.vue'
import UProgress from '@nuxt/ui/components/Progress.vue'
import USwitch from '@nuxt/ui/components/Switch.vue'
import UTabs from '@nuxt/ui/components/Tabs.vue'
import { api } from '../api'
import { errorMessage, useNotifications } from '../composables/useNotifications'
import type { ModelStatus, Settings, VisionConfig, VisionStatus } from '../types'
const props = defineProps<{ settings: Settings; model: ModelStatus | null; assetCount: number }>()
const open = defineModel<boolean>('open', { required: true })
const settingsTab = defineModel<string>('tab', { required: true })
const vision = defineModel<VisionStatus | null>('vision', { required: true })
const emit = defineEmits<{ saved: [settings: Settings]; refresh: []; index: []; describe: [] }>()
const draft = ref({ ...props.settings })
const settingsSaving = ref(false)
const importingModels = ref(false)
const notify = useNotifications()
const visionConfig = ref<VisionConfig>({ baseUrl: '', model: '', autoDescribe: false })
const visionKey = ref(''),
  clearVisionKey = ref(false),
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
      !!visionKey.value ||
      clearVisionKey.value),
)

const settingsTabs = [
  { label: '复制与资料库', value: 'general' },
  { label: '本地搜索模型', value: 'models' },
  { label: '图片理解 API', value: 'vision' },
]

watch(open, (opened) => {
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
    notify(errorMessage(error), true)
  } finally {
    settingsSaving.value = false
  }
}
async function setupModels() {
  try {
    await api.installModels()
    emit('refresh')
  } catch (error) {
    notify(errorMessage(error), true)
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

async function pauseIndex() {
  try {
    await api.pauseIndex()
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
    await api.saveVisionConfig(
      { ...visionConfig.value },
      clearVisionKey.value ? '' : visionKey.value || null,
    )
    visionKey.value = ''
    clearVisionKey.value = false
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

async function pauseVision() {
  try {
    await api.pauseVision()
    emit('refresh')
  } catch (error) {
    notify(errorMessage(error), true)
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
        <UFormField
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
        <UAlert
          color="neutral"
          variant="subtle"
          icon="i-lucide-info"
          description="原图始终保留。GIF 缩放保留动画；动态 WebP / APNG 缩放后以 GIF 复制，颜色和透明效果可能变化。原尺寸复制保留原文件格式。"
        />
      </div>
      <div v-else-if="settingsTab === 'models'" class="space-y-4">
        <div class="model-item">
          <UIcon name="i-lucide-scan-text" class="size-5 shrink-0 text-primary" />
          <div class="flex-1">
            <h3 class="text-sm font-medium">图片文字识别</h3>
            <p class="mt-1 text-xs text-muted">PP-OCRv5 mobile · 中文 / 英文 / 日文</p>
          </div>
          <UBadge color="neutral" variant="subtle" size="xs">
            {{
              model && !model.missing.some((name) => name.startsWith('ocr-')) ? '已安装' : '待安装'
            }}
          </UBadge>
        </div>
        <div class="model-item">
          <UIcon name="i-lucide-text-search" class="size-5 shrink-0 text-primary" />
          <div class="flex-1">
            <h3 class="text-sm font-medium">描述语义检索</h3>
            <p class="mt-1 text-xs text-muted">Qwen3-Embedding-0.6B · 多语言 · 本地推理</p>
          </div>
          <UBadge color="neutral" variant="subtle" size="xs">
            {{
              model && !model.missing.some((name) => name.startsWith('qwen3-'))
                ? '已安装'
                : '待安装'
            }}
          </UBadge>
        </div>
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
          <UButton variant="soft" size="sm" icon="i-lucide-pause" @click="pauseIndex()">
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
        <div class="flex flex-wrap gap-2">
          <UButton
            :variant="model?.ready ? 'outline' : 'solid'"
            icon="i-lucide-folder-open"
            :loading="importingModels"
            :disabled="model?.job.running"
            @click="importModels()"
          >
            {{ importingModels ? '选择模型文件…' : '导入本地模型' }}
          </UButton>
          <UButton
            v-if="model?.ready"
            icon="i-lucide-scan-text"
            :disabled="model.job.running || importingModels"
            @click="emit('index')"
          >
            继续处理待索引表情
          </UButton>
        </div>
        <p class="text-xs text-muted">可多选模型文件。导入与推理均不联网，原文件保留。</p>
        <p v-if="model?.missing.length" class="text-xs break-words text-muted">
          还需导入：{{ model.missing.join('、') }}
        </p>
        <details class="model-location">
          <summary>模型保存位置与离线安装</summary>
          <code>{{ model?.directory || '桌面应用数据目录 / models' }}</code>
          <p>
            支持 README 列出的 PP-OCRv5 mobile、Qwen3-Embedding-0.6B INT8 与配套
            tokenizer，无需重命名。
          </p>
        </details>
        <details v-if="!model?.ready" class="model-location">
          <summary>可选：联网下载</summary>
          <p>点击下方按钮时连接 ModelScope 和 Hugging Face，完整模型约 647 MB，图片不会上传。</p>
          <UButton
            color="neutral"
            variant="outline"
            icon="i-lucide-download"
            :disabled="model?.job.running || importingModels"
            @click="setupModels()"
          >
            联网下载模型
          </UButton>
        </details>
      </div>
      <div v-else class="space-y-4">
        <p class="text-xs text-muted">
          生成时会将压缩图片发送到所配置的 API，可能产生费用。OCR 与文字检索仍在本地运行。
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
            description="密钥仅本次启动有效，不写入磁盘。本机免密服务可留空。"
          >
            <UInput
              v-model="visionKey"
              class="w-full"
              type="password"
              autocomplete="off"
              :placeholder="vision?.hasApiKey ? '已设置，留空保留' : '输入 API Key'"
              :disabled="clearVisionKey || model?.vision.running || visionTesting || visionSaving"
            />
          </UFormField>
          <UCheckbox
            v-if="vision?.hasApiKey"
            v-model="clearVisionKey"
            label="清除已保存的会话密钥"
            :disabled="model?.vision.running || visionTesting || visionSaving"
          />
          <UFormField
            label="导入后自动生成描述"
            description="仅自动处理新导入的图片。已有图片可用下方批量生成。"
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
        <div class="space-y-3 border-t border-default pt-4">
          <div v-if="model?.vision.running" class="space-y-3">
            <div class="flex justify-between gap-3 text-xs text-muted">
              <span>{{ model.vision.message }}</span>
              <span>{{ model.vision.done }} / {{ model.vision.total }}</span>
            </div>
            <UProgress
              :model-value="model.vision.done"
              :max="Math.max(1, model.vision.total)"
              aria-label="图片理解进度"
            />
            <UButton variant="soft" size="sm" icon="i-lucide-pause" @click="pauseVision()">
              暂停图片理解
            </UButton>
          </div>
          <UButton
            v-else
            icon="i-lucide-sparkles"
            :disabled="
              !vision?.active || visionDirty || visionSaving || visionTesting || !assetCount
            "
            @click="emit('describe')"
          >
            生成缺失描述 / 继续队列
          </UButton>
          <p
            v-if="model?.vision.message && !model.vision.running"
            role="status"
            class="text-xs text-muted"
          >
            {{ model.vision.message }}
          </p>
          <UAlert
            v-if="model?.vision.error"
            role="alert"
            color="error"
            variant="subtle"
            :description="model.vision.error"
          />
        </div>
      </div>
    </template>
  </UModal>
</template>
