import { convertFileSrc, invoke as tauriInvoke, isTauri } from '@tauri-apps/api/core'
import type { InvokeArgs } from '@tauri-apps/api/core'
import { getCurrentWebview } from '@tauri-apps/api/webview'
import type { DragDropEvent } from '@tauri-apps/api/webview'
import type {
  Asset,
  Group,
  ImportReport,
  ModelStatus,
  SearchMode,
  Settings,
  Snapshot,
  VisionConfig,
  VisionStatus,
  TaskKind,
  TaskAction,
  ClearKind,
  CopyCacheUsage,
} from './types'

export const isDesktop = isTauri()
export const fileUrl = (path: string) => (isDesktop ? convertFileSrc(path) : path)
function invoke<T>(command: string, args?: InvokeArgs): Promise<T> {
  if (!isDesktop)
    return Promise.reject(new Error('请在桌面应用中使用此功能。运行 pnpm tauri dev 打开桌面版。'))
  return tauriInvoke<T>(command, args)
}
export const api = {
  snapshot: () => invoke<Snapshot>('snapshot'),
  invalidateSearches: () => invoke<number>('invalidate_searches'),
  search: (query: string, mode: SearchMode, groupId: number | null, generation: number) =>
    invoke<Asset[] | null>('search', { query, mode, groupId, generation }),
  importFiles: (paths: string[], groupId: number | null) =>
    invoke<ImportReport>('import_files', { paths, groupId }),
  pickImport: (groupId: number | null) => invoke<ImportReport>('pick_import', { groupId }),
  saveAsset: (id: string, description: string, favorite: boolean, groups: number[]) =>
    invoke<void>('save_asset', { id, description, favorite, groups }),
  deleteAsset: (id: string) => invoke<void>('delete_asset', { id }),
  saveGroup: (id: number | null, name: string, color: string) =>
    invoke<Group>('save_group', { id, name, color }),
  deleteGroup: (id: number) => invoke<void>('delete_group', { id }),
  saveSettings: (settings: Settings) => invoke<void>('save_settings', { settings }),
  copy: (id: string, size: number | null) => invoke<string>('copy_asset', { id, size }),
  copyCache: () => invoke<CopyCacheUsage>('copy_cache_status'),
  clearCopyCache: () => invoke<CopyCacheUsage>('clear_copy_cache'),
  models: () => invoke<ModelStatus>('model_status'),
  importModels: () => invoke<boolean>('import_models'),
  task: (kind: TaskKind, action: TaskAction, id: string | null = null) =>
    invoke<void>('control_task', { kind, action, id }),
  clearText: (kind: ClearKind, id: string | null = null) =>
    invoke<number>('clear_text', { kind, id }),
  pauseInstall: () => invoke<void>('pause_install'),
  visionStatus: () => invoke<VisionStatus>('vision_status'),
  saveVisionConfig: (config: VisionConfig) => invoke<void>('save_vision_config', { config }),
  testVision: () => invoke<{ text: string; model: string }>('test_vision'),
  onDrop: (handler: (type: DragDropEvent['type'], paths: string[]) => void) => {
    if (!isDesktop) return
    return getCurrentWebview().onDragDropEvent(({ payload }) =>
      handler(payload.type, 'paths' in payload ? payload.paths : []),
    )
  },
}
