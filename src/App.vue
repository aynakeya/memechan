<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, useTemplateRef, watch } from 'vue'
import type { ContextMenuItem } from '@nuxt/ui'
import { errorMessage, useNotifications } from './composables/useNotifications'
import { sizeLabel } from './format'
import SettingsDialog from './components/SettingsDialog.vue'
import AssetDetails from './components/AssetDetails.vue'
import AssetGrid from './components/AssetGrid.vue'
import { zh_cn } from '@nuxt/ui/locale'
import UApp from '@nuxt/ui/components/App.vue'
import UAlert from '@nuxt/ui/components/Alert.vue'
import UBadge from '@nuxt/ui/components/Badge.vue'
import UButton from '@nuxt/ui/components/Button.vue'
import UFormField from '@nuxt/ui/components/FormField.vue'
import UIcon from '@nuxt/ui/components/Icon.vue'
import UInput from '@nuxt/ui/components/Input.vue'
import UKbd from '@nuxt/ui/components/Kbd.vue'
import UModal from '@nuxt/ui/components/Modal.vue'
import USelect from '@nuxt/ui/components/Select.vue'
import UTabs from '@nuxt/ui/components/Tabs.vue'
import { api, fileUrl, isDesktop } from './api'
import type { Asset, Group, ModelStatus, SearchMode, Settings, VisionStatus } from './types'

const assets = ref<Asset[]>([]),
  groups = ref<Group[]>([])
const settings = ref<Settings>({ copySize: 0, allowUpscale: false, autoIndex: true })
const scope = ref<string>('all'),
  query = ref(''),
  searchMode = ref<SearchMode>('hybrid'),
  results = ref<Asset[] | null>(null)
const loading = ref(false),
  importing = ref(false),
  searching = ref(false),
  copying = ref<string | null>(null)
const selectedId = ref<string | null>(null),
  preview = ref<Asset | null>(null)
const description = ref(''),
  descriptionSaved = ref(''),
  saving = ref(false)
const dragging = ref(false),
  onlyAnimated = ref(false),
  compact = ref(false),
  sort = ref('newest')
const model = ref<ModelStatus | null>(null),
  modal = ref<'group' | 'delete' | 'deleteGroup' | 'custom' | null>(null)
const groupName = ref(''),
  groupColor = ref('#88966d'),
  editingGroup = ref<Group | null>(null)
const settingsOpen = ref(false)
const settingsTab = ref('general'),
  customSize = ref(320),
  menuOpen = ref(false)
const searchError = ref(''),
  pageError = ref('')
const searchInput = useTemplateRef('searchInput')
const vision = ref<VisionStatus | null>(null)
const notify = useNotifications()
const searchTabs = [
  { label: '智能搜索', value: 'hybrid', icon: 'i-lucide-sparkles' },
  { label: '关键词', value: 'keyword' },
  { label: '语义搜索', value: 'semantic' },
]
const sortOptions = [
  { label: '最近添加', value: 'newest' },
  { label: '名称排序', value: 'name' },
]
const modalTitle = computed(() => {
  if (!modal.value) return ''
  return {
    group: editingGroup.value ? '编辑分组' : '新建分组',
    delete: '删除这张表情？',
    deleteGroup: '删除这个分组？',
    custom: '按尺寸复制',
  }[modal.value]
})
const selected = computed(() => assets.value.find((asset) => asset.id === selectedId.value))
const activeGroup = computed(() =>
  groups.value.find((group) => `group:${group.id}` === scope.value),
)
const groupId = computed(() => activeGroup.value?.id ?? null)
const scopeName = computed(
  () =>
    activeGroup.value?.name ??
    { all: '全部表情', favorites: '我的收藏', recent: '最近添加', ungrouped: '未分组' }[
      scope.value
    ] ??
    '全部表情',
)
const favorites = computed(() => assets.value.filter((asset) => asset.favorite).length)
const ungrouped = computed(() => assets.value.filter((asset) => !asset.groups.length).length)
const visible = computed(() => {
  let list = [...(results.value ?? assets.value)]
  if (scope.value === 'favorites') list = list.filter((asset) => asset.favorite)
  else if (scope.value === 'ungrouped') list = list.filter((asset) => !asset.groups.length)
  else if (scope.value === 'recent')
    list = list.filter((asset) => asset.createdAt > Date.now() - 7 * 86400000)
  else if (groupId.value !== null)
    list = list.filter((asset) => asset.groups.includes(groupId.value!))
  if (onlyAnimated.value) list = list.filter((asset) => asset.animated)
  if (!query.value.trim())
    list.sort((a, b) =>
      sort.value === 'name' ? a.name.localeCompare(b.name, 'zh-CN') : b.createdAt - a.createdAt,
    )
  return list
})
const sizes = [0, 128, 256, 512]
let searchTimer: ReturnType<typeof setTimeout>,
  pollTimer: ReturnType<typeof setInterval>,
  searchSequence = 0
let unlistenDrop: (() => void) | undefined
let refreshingModels = false
async function refresh() {
  const data = await api.snapshot()
  assets.value = data.assets
  groups.value = data.groups
  settings.value = data.settings
  if (selectedId.value && !assets.value.some((asset) => asset.id === selectedId.value))
    selectedId.value = null
}
async function refreshLibrary() {
  await refresh()
  if (query.value.trim()) await runSearch()
}
async function refreshModels() {
  if (refreshingModels) return
  refreshingModels = true
  try {
    const previous = model.value
    model.value = await api.models()
    if (previous) {
      const changed = (['job', 'import', 'vision'] as const).some(
        (key) =>
          previous[key].done !== model.value![key].done ||
          previous[key].running !== model.value![key].running,
      )
      if (changed) await refreshLibrary()
      for (const key of ['job', 'vision'] as const) {
        const after = model.value[key]
        if (previous[key].running && !after.running && after.error) notify(after.error, true)
      }
    }
    pageError.value = ''
  } catch (error) {
    pageError.value = errorMessage(error)
  } finally {
    refreshingModels = false
  }
}
async function runSearch() {
  const sequence = ++searchSequence
  searchError.value = ''
  const text = query.value.trim()
  if (!text) {
    results.value = null
    searching.value = false
    return
  }
  searching.value = true
  try {
    const found = await api.search(text, searchMode.value, groupId.value)
    if (sequence === searchSequence) results.value = found
  } catch (error) {
    if (sequence === searchSequence) {
      searchError.value = errorMessage(error)
      results.value = []
    }
  } finally {
    if (sequence === searchSequence) searching.value = false
  }
}
watch([query, searchMode, scope], () => {
  ++searchSequence
  clearTimeout(searchTimer)
  searchTimer = setTimeout(runSearch, 250)
})
watch(selectedId, () => {
  description.value = selected.value?.description ?? ''
  descriptionSaved.value = description.value
})
async function closeDetails() {
  if (await saveDetails()) selectedId.value = null
}
async function selectAsset(asset: Asset) {
  if (selectedId.value === asset.id) return
  if (description.value !== descriptionSaved.value && !(await saveDetails())) return
  selectedId.value = asset.id
}
async function saveDetails() {
  if (saving.value) return false
  const asset = selected.value
  if (!asset || description.value === descriptionSaved.value) return true
  saving.value = true
  const text = description.value
  try {
    await api.saveAsset(asset.id, text, asset.favorite, asset.groups)
    if (asset.id === selectedId.value) descriptionSaved.value = text
    await refreshLibrary()
    return description.value === descriptionSaved.value
  } catch (error) {
    notify(errorMessage(error), true)
    return false
  } finally {
    saving.value = false
  }
}
async function favorite(asset: Asset) {
  if (saving.value) return
  saving.value = true
  const text = asset.id === selectedId.value ? description.value : asset.description
  try {
    await api.saveAsset(asset.id, text, !asset.favorite, asset.groups)
    if (asset.id === selectedId.value) descriptionSaved.value = text
    await refreshLibrary()
  } catch (error) {
    notify(errorMessage(error), true)
  } finally {
    saving.value = false
  }
}
async function toggleGroup(id: number) {
  if (!selected.value || saving.value) return
  saving.value = true
  const asset = selected.value
  const text = description.value
  const membership = asset.groups.includes(id)
    ? asset.groups.filter((group) => group !== id)
    : [...asset.groups, id]
  try {
    await api.saveAsset(asset.id, text, asset.favorite, membership)
    if (asset.id === selectedId.value) descriptionSaved.value = text
    await refreshLibrary()
  } catch (error) {
    notify(errorMessage(error), true)
  } finally {
    saving.value = false
  }
}
async function importAssets(paths?: string[]) {
  if (importing.value) return
  importing.value = true
  try {
    const report = paths
      ? await api.importFiles(paths, groupId.value)
      : await api.pickImport(groupId.value)
    if (report.added || report.duplicates)
      notify(
        `已添加 ${report.added} 张表情${report.duplicates ? `，${report.duplicates} 张重复图片已合并` : ''}`,
      )
    if (report.errors.length) notify(report.errors.join('\n'), true)
    await refreshLibrary()
  } catch (error) {
    notify(errorMessage(error), true)
  } finally {
    importing.value = false
    dragging.value = false
  }
}
async function copy(asset: Asset, size: number | null = null) {
  if (copying.value) return
  copying.value = asset.id
  try {
    const detail = await api.copy(asset.id, size)
    notify(detail)
  } catch (error) {
    notify(errorMessage(error), true)
  } finally {
    copying.value = null
  }
}
function assetMenu(asset: Asset): ContextMenuItem[][] {
  return [
    [
      { label: '复制尺寸', type: 'label' },
      ...sizes.map((size) => ({
        label: sizeLabel(size),
        icon: settings.value.copySize === size ? 'i-lucide-check' : 'i-lucide-copy',
        onSelect: () => void copy(asset, size),
      })),
      {
        label: '自定义尺寸…',
        icon: 'i-lucide-scaling',
        onSelect: () => void contextAction(asset, true),
      },
    ],
    [
      {
        label: '预览原图',
        icon: 'i-lucide-image',
        kbds: ['space'],
        onSelect: () => {
          preview.value = asset
        },
      },
      {
        label: '编辑描述与分组',
        icon: 'i-lucide-pencil',
        onSelect: () => void contextAction(asset),
      },
    ],
  ]
}
async function contextAction(asset: Asset, custom = false) {
  await selectAsset(asset)
  if (custom && selectedId.value === asset.id) modal.value = 'custom'
}
function openGroup(group: Group | null = null) {
  editingGroup.value = group
  groupName.value = group?.name ?? ''
  groupColor.value = group?.color ?? '#88966d'
  modal.value = 'group'
}
async function saveGroup() {
  try {
    await api.saveGroup(editingGroup.value?.id ?? null, groupName.value, groupColor.value)
    await refresh()
    modal.value = null
    notify('分组已保存')
  } catch (error) {
    notify(errorMessage(error), true)
  }
}
async function removeGroup() {
  if (!activeGroup.value) return
  try {
    await api.deleteGroup(activeGroup.value.id)
    scope.value = 'all'
    modal.value = null
    await refresh()
    notify('已删除分组，表情仍保留在资料库中')
  } catch (error) {
    notify(errorMessage(error), true)
  }
}
async function removeAsset() {
  if (!selected.value) return
  try {
    await api.deleteAsset(selected.value.id)
    modal.value = null
    selectedId.value = null
    await refreshLibrary()
    notify('已从资料库删除')
  } catch (error) {
    notify(errorMessage(error), true)
  }
}
async function startIndex(id: string | null = null) {
  try {
    await api.index(id)
    await refreshModels()
  } catch (error) {
    notify(errorMessage(error), true)
  }
}
function openSettings(tab = 'general') {
  settingsTab.value = tab
  settingsOpen.value = true
}
async function describe(id: string | null = null) {
  try {
    await api.describe(id)
    await refresh()
    await refreshModels()
  } catch (error) {
    notify(errorMessage(error), true)
  }
}
function keyboard(event: KeyboardEvent) {
  if (
    event.defaultPrevented ||
    settingsOpen.value ||
    modal.value ||
    preview.value ||
    menuOpen.value
  )
    return
  if ((event.metaKey || event.ctrlKey) && event.key === 'k') {
    event.preventDefault()
    searchInput.value?.inputRef?.focus()
    return
  }
  if (
    (event.target as HTMLElement)?.closest(
      'input,textarea,select,button,[role="combobox"],[role="tab"],[contenteditable]',
    )
  )
    return
  if (selected.value && event.code === 'Space') {
    event.preventDefault()
    preview.value = selected.value
  }
  if (
    selected.value &&
    (event.key === 'Enter' || ((event.metaKey || event.ctrlKey) && event.key === 'c'))
  ) {
    event.preventDefault()
    void copy(selected.value)
  }
}
onMounted(async () => {
  document.addEventListener('keydown', keyboard)
  if (!isDesktop) return
  loading.value = true
  try {
    await refresh()
    await refreshModels()
    vision.value = await api.visionStatus()
    unlistenDrop = await api.onDrop((type, paths) => {
      dragging.value = type === 'enter' || type === 'over'
      if (type === 'drop') void importAssets(paths)
    })
    pollTimer = setInterval(refreshModels, 1600)
  } catch (error) {
    pageError.value = errorMessage(error)
  } finally {
    loading.value = false
  }
})
onBeforeUnmount(() => {
  unlistenDrop?.()
  clearInterval(pollTimer)
  clearTimeout(searchTimer)
  document.removeEventListener('keydown', keyboard)
})
</script>

<template>
  <UApp :locale="zh_cn" :toaster="{ position: 'bottom-right', duration: 3500 }">
    <div class="app-shell" @dragover.prevent @drop.prevent>
      <aside class="sidebar">
        <a
          class="brand"
          href="#"
          @click.prevent="
            () => {
              scope = 'all'
              query = ''
            }
          "
        >
          <img class="brand-mark" src="/memechan-icon.png" alt="" width="40" height="40" />
          <span>Memechan</span>
        </a>
        <p class="nav-label">我的表情库</p>
        <nav class="space-y-1" aria-label="资料库">
          <UButton
            block
            class="justify-start"
            :color="scope === 'all' ? 'primary' : 'neutral'"
            :variant="scope === 'all' ? 'soft' : 'ghost'"
            icon="i-lucide-grid-2x2"
            @click="scope = 'all'"
          >
            全部表情
            <UBadge class="ml-auto" color="neutral" variant="subtle" size="xs">
              {{ assets.length }}
            </UBadge>
          </UButton>
          <UButton
            block
            class="justify-start"
            :color="scope === 'favorites' ? 'primary' : 'neutral'"
            :variant="scope === 'favorites' ? 'soft' : 'ghost'"
            icon="i-lucide-heart"
            @click="scope = 'favorites'"
          >
            我的收藏
            <UBadge class="ml-auto" color="neutral" variant="subtle" size="xs">
              {{ favorites }}
            </UBadge>
          </UButton>
          <UButton
            block
            class="justify-start"
            :color="scope === 'recent' ? 'primary' : 'neutral'"
            :variant="scope === 'recent' ? 'soft' : 'ghost'"
            icon="i-lucide-clock"
            @click="scope = 'recent'"
          >
            最近添加
          </UButton>
          <UButton
            block
            class="justify-start"
            :color="scope === 'ungrouped' ? 'primary' : 'neutral'"
            :variant="scope === 'ungrouped' ? 'soft' : 'ghost'"
            icon="i-lucide-folder"
            @click="scope = 'ungrouped'"
          >
            未分组
            <UBadge class="ml-auto" color="neutral" variant="subtle" size="xs">
              {{ ungrouped }}
            </UBadge>
          </UButton>
        </nav>
        <div class="nav-label flex items-center justify-between">
          我的分组
          <UButton
            color="neutral"
            variant="ghost"
            size="xs"
            icon="i-lucide-plus"
            aria-label="新建分组"
            @click="openGroup()"
          />
        </div>
        <nav class="min-h-0 space-y-1 overflow-auto" aria-label="分组">
          <UButton
            v-for="group in groups"
            :key="group.id"
            block
            class="justify-start"
            :color="groupId === group.id ? 'primary' : 'neutral'"
            :variant="groupId === group.id ? 'soft' : 'ghost'"
            @click="scope = `group:${group.id}`"
          >
            <UIcon name="i-lucide-folder" class="size-4 shrink-0" :style="{ color: group.color }" />
            <span class="truncate">{{ group.name }}</span>
            <span class="ml-auto text-xs text-muted">{{ group.count }}</span>
          </UButton>
          <UButton
            block
            class="justify-start"
            color="neutral"
            variant="ghost"
            icon="i-lucide-plus"
            @click="openGroup()"
          >
            新建分组
          </UButton>
        </nav>
        <div class="mt-auto pt-6">
          <UButton
            block
            class="justify-start"
            color="neutral"
            variant="ghost"
            icon="i-lucide-settings"
            @click="openSettings()"
          >
            偏好设置
          </UButton>
        </div>
      </aside>

      <main class="main-content">
        <UAlert
          v-if="!isDesktop"
          class="mt-5"
          color="warning"
          variant="subtle"
          icon="i-lucide-info"
          description="当前为界面预览。导入、剪贴板和本地模型请在桌面应用中使用。"
        />
        <UAlert
          v-if="pageError"
          role="alert"
          class="mt-5"
          color="error"
          variant="subtle"
          icon="i-lucide-circle-alert"
          :description="pageError"
        />
        <header class="library-heading">
          <h1 class="text-2xl font-semibold text-highlighted">{{ scopeName }}</h1>
          <div class="flex gap-2">
            <UButton
              v-if="activeGroup"
              color="neutral"
              variant="outline"
              icon="i-lucide-pencil"
              @click="openGroup(activeGroup)"
            >
              编辑分组
            </UButton>
            <UButton icon="i-lucide-plus" :loading="importing" @click="importAssets()">
              {{ importing ? '正在导入…' : '添加表情' }}
            </UButton>
          </div>
        </header>

        <section class="space-y-3" aria-label="搜索">
          <UInput
            ref="searchInput"
            v-model="query"
            class="w-full"
            size="lg"
            icon="i-lucide-search"
            :loading="searching"
            aria-label="搜索表情"
            placeholder="搜索文字或描述"
            :ui="{ trailing: 'pr-2' }"
          >
            <template #trailing>
              <UButton
                v-if="query"
                color="neutral"
                variant="ghost"
                size="xs"
                icon="i-lucide-x"
                aria-label="清空搜索"
                @click="query = ''"
              />
              <UKbd v-else value="ctrl" class="text-[10px]">Ctrl K</UKbd>
            </template>
          </UInput>
          <div class="search-options">
            <UTabs
              v-model="searchMode"
              :items="searchTabs"
              :content="false"
              size="sm"
              aria-label="搜索方式"
            />
            <UButton
              v-if="!model?.ready"
              size="xs"
              variant="link"
              color="neutral"
              trailing-icon="i-lucide-arrow-right"
              @click="openSettings('models')"
            >
              配置 OCR 与语义搜索
            </UButton>
            <span v-else class="flex items-center gap-1.5 text-xs text-muted">
              <span class="size-1.5 rounded-full bg-primary"></span>
              本地模型已就绪
            </span>
          </div>
        </section>

        <div class="collection-toolbar">
          <div class="flex flex-wrap items-center gap-2 text-xs text-muted">
            <strong v-if="query" class="font-medium text-default">搜索结果</strong>
            <span>{{ visible.length }} 张</span>
            <span v-if="model?.import.running" role="status">
              导入 {{ model.import.done }} / {{ model.import.total }}
            </span>
            <UButton
              v-if="model?.job.running"
              size="xs"
              variant="link"
              color="neutral"
              @click="openSettings('models')"
            >
              后台处理 {{ model.job.done }} / {{ model.job.total }}
            </UButton>
            <UButton
              v-if="model?.vision.running"
              size="xs"
              variant="link"
              color="neutral"
              @click="openSettings('vision')"
            >
              图片理解 {{ model.vision.done }} / {{ model.vision.total }}
            </UButton>
            <span v-if="query && searchMode === 'hybrid' && !model?.ready">当前使用关键词匹配</span>
          </div>
          <div class="flex shrink-0 items-center gap-2">
            <UButton
              size="sm"
              :color="onlyAnimated ? 'primary' : 'neutral'"
              :variant="onlyAnimated ? 'soft' : 'ghost'"
              icon="i-lucide-play"
              :aria-pressed="onlyAnimated"
              @click="onlyAnimated = !onlyAnimated"
            >
              只看动图
            </UButton>
            <USelect
              v-model="sort"
              :items="sortOptions"
              size="sm"
              class="w-28"
              aria-label="排序方式"
              :disabled="!!query"
              @update:open="menuOpen = $event"
            />
            <UButton
              color="neutral"
              variant="ghost"
              size="sm"
              :icon="compact ? 'i-lucide-grid-3x3' : 'i-lucide-grid-2x2'"
              :aria-label="compact ? '切换为舒适网格' : '切换为紧凑网格'"
              :aria-pressed="compact"
              @click="compact = !compact"
            />
          </div>
        </div>
        <UAlert
          v-if="searchError"
          role="alert"
          color="error"
          variant="subtle"
          icon="i-lucide-circle-alert"
          :description="searchError"
          :actions="[
            {
              label: '查看模型设置',
              color: 'error',
              variant: 'outline',
              onClick: () => openSettings('models'),
            },
          ]"
        />
        <div v-if="loading" class="empty-state">
          <UIcon name="i-lucide-loader-circle" class="size-6 animate-spin text-primary" />
          <p class="mt-3 text-muted">正在加载…</p>
        </div>
        <div v-else-if="!visible.length" class="empty-state">
          <UIcon
            :name="query ? 'i-lucide-search' : 'i-lucide-images'"
            class="mb-4 size-9 text-dimmed"
          />
          <template v-if="query">
            <h2 class="font-medium text-highlighted">
              {{ searchError ? '搜索失败' : '没有匹配的表情' }}
            </h2>
            <p class="mt-2 mb-5 text-sm text-muted">
              {{ searchError ? '请处理上方错误后重试。' : '尝试其他关键词或描述。' }}
            </p>
            <UButton color="neutral" variant="outline" @click="query = ''">查看全部表情</UButton>
          </template>
          <template v-else-if="onlyAnimated">
            <h2 class="font-medium text-highlighted">暂无动图</h2>
            <p class="mt-2 mb-5 text-sm text-muted">支持 GIF、动态 WebP 和 APNG。</p>
            <UButton color="neutral" variant="outline" @click="onlyAnimated = false">
              显示所有格式
            </UButton>
          </template>
          <template v-else>
            <h2 class="font-medium text-highlighted">
              {{ scope === 'favorites' ? '暂无收藏' : '暂无表情' }}
            </h2>
            <p class="mt-2 mb-5 text-sm text-muted">
              {{
                scope === 'favorites'
                  ? '点击表情上的爱心即可收藏。'
                  : '拖入图片，或点击下方按钮添加。'
              }}
            </p>
            <UButton
              v-if="scope !== 'favorites'"
              icon="i-lucide-image-plus"
              :loading="importing"
              @click="importAssets()"
            >
              选择图片
            </UButton>
          </template>
        </div>
        <AssetGrid
          v-else
          :assets="visible"
          :selected-id="selectedId"
          :copying="copying"
          :compact="compact"
          :importing="importing"
          :menu-items="assetMenu"
          @select="selectAsset"
          @copy="copy($event)"
          @preview="preview = $event"
          @favorite="favorite"
          @import="importAssets()"
          @menu-open="menuOpen = $event"
        />
        <footer class="library-footer">
          <UIcon name="i-lucide-info" class="size-3.5" />
          双击复制 · 空格预览 · 右键选择尺寸
        </footer>
      </main>

      <AssetDetails
        v-if="selected"
        v-model="description"
        :asset="selected"
        :groups="groups"
        :model="model"
        :vision-active="!!vision?.active"
        :copy-size="settings.copySize"
        :copying="copying"
        :saving="saving"
        :description-saved="descriptionSaved"
        @close="closeDetails()"
        @preview="preview = selected"
        @copy="copy(selected)"
        @favorite="favorite(selected)"
        @save="saveDetails()"
        @index="startIndex(selected.id)"
        @describe="describe(selected.id)"
        @settings="openSettings($event)"
        @create-group="openGroup()"
        @toggle-group="toggleGroup($event)"
        @remove="modal = 'delete'"
      />
    </div>

    <div v-if="dragging" class="drop-overlay">
      <div
        class="rounded-xl border-2 border-dashed border-primary bg-default p-12 text-center shadow-xl"
      >
        <UIcon name="i-lucide-image-plus" class="mb-4 size-10 text-primary" />
        <h2 class="text-lg font-semibold">松开以添加表情</h2>
        <p v-if="activeGroup" class="mt-2 text-muted">添加到「{{ activeGroup.name }}」</p>
      </div>
    </div>

    <UModal
      :open="!!preview"
      title="原图预览"
      :description="
        preview
          ? `${preview.name} · ${preview.width} × ${preview.height} · ${preview.format.toUpperCase()}`
          : undefined
      "
      :close="{ 'aria-label': '关闭预览' }"
      :ui="{
        content: 'max-w-4xl',
        body: 'flex items-center justify-center bg-muted min-h-48',
        footer: 'justify-end',
      }"
      @update:open="
        (open) => {
          if (!open) preview = null
        }
      "
    >
      <template #body>
        <img
          v-if="preview"
          :src="fileUrl(preview.path)"
          :alt="preview.name"
          class="max-h-[65vh] max-w-full object-contain"
        />
      </template>
      <template #footer>
        <UButton
          v-if="preview"
          icon="i-lucide-copy"
          :loading="copying === preview.id"
          @click="copy(preview)"
        >
          复制表情
        </UButton>
      </template>
    </UModal>

    <SettingsDialog
      v-model:open="settingsOpen"
      v-model:tab="settingsTab"
      v-model:vision="vision"
      :settings="settings"
      :model="model"
      :asset-count="assets.length"
      @saved="settings = $event"
      @refresh="refreshModels()"
      @index="startIndex()"
      @describe="describe()"
    />
    <UModal
      :open="!!modal"
      :title="modalTitle"
      :close="{ 'aria-label': '关闭弹窗' }"
      :ui="{ content: 'max-w-md' }"
      @update:open="
        (open) => {
          if (!open) modal = null
        }
      "
    >
      <template #body>
        <form v-if="modal === 'group'" class="space-y-5" @submit.prevent="saveGroup">
          <UFormField label="分组名称" name="group-name" required>
            <UInput
              v-model="groupName"
              class="w-full"
              placeholder="例如：打工日常"
              :maxlength="40"
              required
              autofocus
            />
          </UFormField>
          <fieldset>
            <legend class="mb-3 text-sm font-medium">分组颜色</legend>
            <div class="flex gap-2">
              <UButton
                v-for="color in ['#88966d', '#c4906f', '#819eaf', '#ae8caf', '#d2b359', '#799f96']"
                :key="color"
                type="button"
                class="size-8 justify-center"
                :style="{ backgroundColor: color, color: 'white' }"
                :aria-label="`选择颜色 ${color}`"
                :aria-pressed="groupColor === color"
                @click="groupColor = color"
              >
                <UIcon v-if="groupColor === color" name="i-lucide-check" class="size-4" />
              </UButton>
            </div>
          </fieldset>
          <div class="flex justify-end gap-2 pt-3">
            <UButton
              v-if="editingGroup"
              type="button"
              color="error"
              variant="ghost"
              class="mr-auto"
              @click="
                () => {
                  scope = `group:${editingGroup!.id}`
                  modal = 'deleteGroup'
                }
              "
            >
              删除分组
            </UButton>
            <UButton type="button" color="neutral" variant="outline" @click="modal = null">
              取消
            </UButton>
            <UButton type="submit" :disabled="!groupName.trim()">保存分组</UButton>
          </div>
        </form>
        <form
          v-else-if="modal === 'custom'"
          class="space-y-5"
          @submit.prevent="
            () => {
              if (selected) copy(selected, customSize)
              modal = null
            }
          "
        >
          <UFormField
            label="最长边（像素）"
            name="custom-size"
            description="另一边按比例缩放，动图保留动画。"
          >
            <UInput
              v-model.number="customSize"
              class="w-full"
              type="number"
              :min="16"
              :max="4096"
              required
              autofocus
            />
          </UFormField>
          <div class="flex justify-end gap-2">
            <UButton type="button" color="neutral" variant="outline" @click="modal = null">
              取消
            </UButton>
            <UButton type="submit">按此尺寸复制</UButton>
          </div>
        </form>
        <template v-else-if="modal">
          <p class="text-sm leading-relaxed text-muted">
            {{
              modal === 'delete'
                ? '将删除资料库中的原图、描述和索引。最初导入的来源文件不受影响。此操作无法撤销。'
                : '只删除分组，组内的表情仍保留在「全部表情」中。'
            }}
          </p>
          <div class="mt-6 flex justify-end gap-2">
            <UButton color="neutral" variant="outline" @click="modal = null">取消</UButton>
            <UButton color="error" @click="modal === 'delete' ? removeAsset() : removeGroup()">
              确认删除
            </UButton>
          </div>
        </template>
      </template>
    </UModal>
  </UApp>
</template>
