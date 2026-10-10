<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, useTemplateRef, watch } from 'vue'
import type { ContextMenuItem } from '@nuxt/ui'
import { errorMessage, useNotifications } from './composables/useNotifications'
import { sizeLabel } from './format'
import TaskManager from './components/TaskManager.vue'
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
import type {
  Asset,
  Group,
  ModelStatus,
  SearchMode,
  Settings,
  VisionStatus,
  TaskKind,
  ClearKind,
} from './types'

const assets = ref<Asset[]>([]),
  groups = ref<Group[]>([])
const settings = ref<Settings>({
  copySize: 0,
  allowUpscale: false,
  autoIndex: true,
  ocrEnabled: true,
  embeddingModel: 'qwen3-0.6b',
})
const scope = ref<string>('all'),
  query = ref(''),
  searchComposing = ref(false),
  searchMode = ref<SearchMode>('hybrid'),
  results = ref<Asset[] | null>(null)
const loading = ref(false),
  importing = ref(false),
  searching = ref(false),
  copying = ref<string | null>(null)
const selectedId = ref<string | null>(null),
  preview = ref<Asset | null>(null)
const smallWindowMedia = window.matchMedia('(max-width: 799px)')
const isSmallWindow = ref(smallWindowMedia.matches)
const detailsOpen = ref(false)
const mainContent = useTemplateRef<HTMLElement>('mainContent')
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
const tasksOpen = ref(false)
const clearTarget = ref<{ kind: ClearKind; id: string | null } | null>(null)
const clearing = ref(false)
const clearTitle = computed(() =>
  clearTarget.value
    ? '清除' +
      (clearTarget.value.id ? '这张图片的' : '全部') +
      { ocr: ' OCR 文字？', vision: '图片描述？', index: '文字向量索引？' }[clearTarget.value.kind]
    : '',
)
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
const showingDetailPage = computed(
  () => isSmallWindow.value && detailsOpen.value && !!selected.value,
)
function windowSizeChanged(event: MediaQueryListEvent) {
  isSmallWindow.value = event.matches
}
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
  // Typing only changes the draft; recompute the grid when search results arrive.
  if (results.value === null)
    list.sort((a, b) =>
      sort.value === 'name' ? a.name.localeCompare(b.name, 'zh-CN') : b.createdAt - a.createdAt,
    )
  return list
})
const sizes = [0, 128, 256, 512]
let searchTimer: ReturnType<typeof setTimeout>,
  pollTimer: ReturnType<typeof setInterval>,
  searchSequence = 0
let listScrollTop = 0
let unlistenDrop: (() => void) | undefined
let refreshingModels = false
let libraryRevision = -1
async function refresh() {
  const data = await api.snapshot()
  libraryRevision = data.revision
  assets.value = data.assets
  groups.value = data.groups
  settings.value = data.settings
  if (selectedId.value && !assets.value.some((asset) => asset.id === selectedId.value))
    selectedId.value = null
}
async function refreshLibrary() {
  await refresh()
  if (query.value.trim()) scheduleSearch()
}
async function refreshModels() {
  if (refreshingModels) return
  refreshingModels = true
  try {
    const previous = model.value
    model.value = await api.models()
    if (previous) {
      if (model.value.libraryRevision !== libraryRevision) await refreshLibrary()
      for (const key of ['job', 'ocr', 'index', 'vision'] as const) {
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
function scheduleSearch() {
  ++searchSequence
  clearTimeout(searchTimer)
  searching.value = false
  if (!searchComposing.value) searchTimer = setTimeout(runSearch, 350)
}
watch([query, searchMode, scope, searchComposing], scheduleSearch)
watch(showingDetailPage, async (open) => {
  const main = mainContent.value!
  if (open) listScrollTop = main.scrollTop
  else {
    await nextTick()
    main.scrollTop = listScrollTop
  }
})
watch(selectedId, (id) => {
  if (!id) detailsOpen.value = false
  description.value = selected.value?.description ?? ''
  descriptionSaved.value = description.value
})
async function closeDetails() {
  if (!(await saveDetails())) return
  detailsOpen.value = false
  await nextTick()
  mainContent.value
    ?.querySelector<HTMLElement>('.asset-card.selected')
    ?.focus({ preventScroll: true })
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
  else if (!custom && selectedId.value === asset.id) {
    detailsOpen.value = true
    if (isSmallWindow.value) {
      await nextTick()
      document.querySelector<HTMLElement>('.details-panel')?.focus()
    }
  }
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
async function generate(kind: TaskKind, id: string) {
  try {
    await api.task(kind, 'generate', id)
    await refreshLibrary()
    await refreshModels()
  } catch (error) {
    notify(errorMessage(error), true)
  }
}
function openSettings(tab = 'general') {
  tasksOpen.value = false
  settingsTab.value = tab
  settingsOpen.value = true
}
function openTasks() {
  settingsOpen.value = false
  tasksOpen.value = true
  void refreshModels()
}
async function clearText() {
  if (!clearTarget.value || clearing.value) return
  clearing.value = true
  try {
    await api.clearText(clearTarget.value.kind, clearTarget.value.id)
    clearTarget.value = null
    await refreshLibrary()
    await refreshModels()
    notify('已清除')
  } catch (error) {
    notify(errorMessage(error), true)
  } finally {
    clearing.value = false
  }
}
async function keyboard(event: KeyboardEvent) {
  if (
    event.defaultPrevented ||
    settingsOpen.value ||
    tasksOpen.value ||
    clearTarget.value ||
    modal.value ||
    preview.value ||
    menuOpen.value
  )
    return
  if ((event.metaKey || event.ctrlKey) && event.key === 'k') {
    event.preventDefault()
    if (showingDetailPage.value) {
      await closeDetails()
      if (showingDetailPage.value) return
    }
    searchInput.value?.inputRef?.focus()
    return
  }
  if (event.key === 'Escape' && showingDetailPage.value) {
    event.preventDefault()
    await closeDetails()
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
  smallWindowMedia.addEventListener('change', windowSizeChanged)
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
  smallWindowMedia.removeEventListener('change', windowSizeChanged)
  unlistenDrop?.()
  clearInterval(pollTimer)
  clearTimeout(searchTimer)
  document.removeEventListener('keydown', keyboard)
})
</script>

<template>
  <UApp :locale="zh_cn" :toaster="{ position: 'bottom-right', duration: 3500 }">
    <div class="app-shell" @dragover.prevent @drop.prevent>
      <aside v-show="!showingDetailPage" class="sidebar">
        <a
          class="brand"
          href="#"
          aria-label="Memechan · 全部表情"
          title="Memechan · 全部表情"
          @click.prevent="
            () => {
              scope = 'all'
              query = ''
            }
          "
        >
          <img class="brand-mark" src="/memechan-icon.png" alt="" width="40" height="40" />
          <span class="sidebar-label">Memechan</span>
        </a>
        <p class="nav-label">我的表情库</p>
        <nav class="space-y-1" aria-label="资料库">
          <UButton
            block
            class="justify-start"
            :color="scope === 'all' ? 'primary' : 'neutral'"
            :variant="scope === 'all' ? 'soft' : 'ghost'"
            icon="i-lucide-grid-2x2"
            aria-label="全部表情"
            title="全部表情"
            @click="scope = 'all'"
          >
            <span class="sidebar-label">全部表情</span>
            <UBadge class="sidebar-count ml-auto" color="neutral" variant="subtle" size="xs">
              {{ assets.length }}
            </UBadge>
          </UButton>
          <UButton
            block
            class="justify-start"
            :color="scope === 'favorites' ? 'primary' : 'neutral'"
            :variant="scope === 'favorites' ? 'soft' : 'ghost'"
            icon="i-lucide-heart"
            aria-label="我的收藏"
            title="我的收藏"
            @click="scope = 'favorites'"
          >
            <span class="sidebar-label">我的收藏</span>
            <UBadge class="sidebar-count ml-auto" color="neutral" variant="subtle" size="xs">
              {{ favorites }}
            </UBadge>
          </UButton>
          <UButton
            block
            class="justify-start"
            :color="scope === 'recent' ? 'primary' : 'neutral'"
            :variant="scope === 'recent' ? 'soft' : 'ghost'"
            icon="i-lucide-clock"
            aria-label="最近添加"
            title="最近添加"
            @click="scope = 'recent'"
          >
            <span class="sidebar-label">最近添加</span>
          </UButton>
          <UButton
            block
            class="justify-start"
            :color="scope === 'ungrouped' ? 'primary' : 'neutral'"
            :variant="scope === 'ungrouped' ? 'soft' : 'ghost'"
            icon="i-lucide-folder"
            aria-label="未分组"
            title="未分组"
            @click="scope = 'ungrouped'"
          >
            <span class="sidebar-label">未分组</span>
            <UBadge class="sidebar-count ml-auto" color="neutral" variant="subtle" size="xs">
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
            :aria-label="group.name"
            :title="group.name"
            @click="scope = `group:${group.id}`"
          >
            <UIcon name="i-lucide-folder" class="size-4 shrink-0" :style="{ color: group.color }" />
            <span class="sidebar-label truncate">{{ group.name }}</span>
            <span class="sidebar-count ml-auto text-xs text-muted">{{ group.count }}</span>
          </UButton>
          <UButton
            block
            class="justify-start"
            color="neutral"
            variant="ghost"
            icon="i-lucide-plus"
            aria-label="新建分组"
            title="新建分组"
            @click="openGroup()"
          >
            <span class="sidebar-label">新建分组</span>
          </UButton>
        </nav>
        <div class="sidebar-footer mt-auto shrink-0 space-y-1 pt-6">
          <UButton
            block
            class="justify-start"
            color="neutral"
            variant="ghost"
            icon="i-lucide-list-todo"
            aria-label="任务管理"
            title="任务管理"
            @click="openTasks()"
          >
            <span class="sidebar-label">任务管理</span>
          </UButton>
          <UButton
            block
            class="justify-start"
            color="neutral"
            variant="ghost"
            icon="i-lucide-settings"
            aria-label="偏好设置"
            title="偏好设置"
            @click="openSettings()"
          >
            <span class="sidebar-label">偏好设置</span>
          </UButton>
        </div>
      </aside>

      <main v-show="!showingDetailPage" ref="mainContent" class="main-content">
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

        <section class="search-panel space-y-3" aria-label="搜索">
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
            @compositionstart="searchComposing = true"
            @compositionend="searchComposing = false"
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
            <!-- Measure the tab indicator only while the list page is visible. -->
            <UTabs
              v-if="!showingDetailPage"
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
              语义模型已就绪
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
              v-if="model?.ocr.running || model?.index.running || model?.vision.running"
              size="xs"
              variant="link"
              color="neutral"
              @click="openTasks()"
            >
              后台任务处理中
            </UButton>
            <span v-if="query && searchMode === 'hybrid' && !model?.ready">当前使用关键词匹配</span>
          </div>
          <div class="flex flex-wrap items-center gap-2">
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
            <UButton
              size="sm"
              :color="detailsOpen ? 'primary' : 'neutral'"
              :variant="detailsOpen ? 'soft' : 'outline'"
              icon="i-lucide-panel-right"
              :aria-pressed="detailsOpen"
              :disabled="!selected"
              @click="selected && (detailsOpen ? closeDetails() : contextAction(selected))"
            >
              详情
            </UButton>
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
          :scroll-element="mainContent"
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
        v-if="selected && detailsOpen"
        v-model="description"
        :page="isSmallWindow"
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
        @index="generate('index', selected.id)"
        @ocr="generate('ocr', selected.id)"
        @clear="clearTarget = { kind: $event, id: selected.id }"
        @describe="generate('vision', selected.id)"
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
      :fullscreen="isSmallWindow"
      :ui="{
        content: isSmallWindow ? 'h-dvh max-w-none' : 'h-[calc(100dvh-4rem)] max-w-4xl',
        wrapper: 'min-w-0 flex-1 pr-8',
        description: 'truncate',
        body: 'flex min-h-0 items-center justify-center overflow-hidden bg-muted p-2 sm:p-4',
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
          class="h-full w-full min-h-0 object-contain"
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

    <TaskManager
      v-model:open="tasksOpen"
      :model="model"
      :vision-active="!!vision?.active"
      :asset-count="assets.length"
      @refresh="refreshModels()"
      @settings="openSettings($event)"
      @clear="clearTarget = { kind: $event, id: null }"
    />
    <UModal
      :open="!!clearTarget"
      :title="clearTitle"
      :dismissible="!clearing"
      :close="!clearing"
      :ui="{ content: 'max-w-md', footer: 'justify-end' }"
      @update:open="
        (open) => {
          if (!open) clearTarget = null
        }
      "
    >
      <template #body>
        <p v-if="clearTarget?.kind === 'index'" class="text-sm text-muted">
          仅删除文字向量索引，保留原图、OCR
          文字和所有描述。清除后暂停这些图片的自动索引，需在任务管理中点击“建立缺失索引”重建；关键词搜索仍可使用。
        </p>
        <p v-else class="text-sm text-muted">
          仅清除{{
            clearTarget?.kind === 'ocr' ? '本地 OCR 文字' : 'API 生成的图片描述'
          }}，保留原图与手写描述。相关索引将更新，已清除内容不会自动重新生成。
        </p>
      </template>
      <template #footer>
        <UButton color="neutral" variant="outline" :disabled="clearing" @click="clearTarget = null">
          取消
        </UButton>
        <UButton color="error" :loading="clearing" @click="clearText()">确认清除</UButton>
      </template>
    </UModal>
    <SettingsDialog
      v-model:open="settingsOpen"
      v-model:tab="settingsTab"
      v-model:vision="vision"
      :settings="settings"
      :model="model"
      @saved="settings = $event"
      @refresh="refreshModels()"
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
