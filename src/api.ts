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
  search: (query: string, mode: SearchMode, groupId: number | null) =>
    invoke<Asset[]>('search', { query, mode, groupId }),
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
  models: () => invoke<ModelStatus>('model_status'),
  installModels: () => invoke<void>('install_models'),
  importModels: () => invoke<boolean>('import_models'),
  index: (id: string | null = null) => invoke<void>('start_index', { id }),
  pauseIndex: () => invoke<void>('pause_index'),
  visionStatus: () => invoke<VisionStatus>('vision_status'),
  saveVisionConfig: (config: VisionConfig, apiKey: string | null) =>
    invoke<void>('save_vision_config', { config, apiKey }),
  testVision: () => invoke<{ text: string; model: string }>('test_vision'),
  describe: (id: string | null = null) => invoke<void>('start_vision', { id }),
  pauseVision: () => invoke<void>('pause_vision'),
  onDrop: (handler: (type: DragDropEvent['type'], paths: string[]) => void) => {
    if (!isDesktop) return
    return getCurrentWebview().onDragDropEvent(({ payload }) =>
      handler(payload.type, 'paths' in payload ? payload.paths : []),
    )
  },
}
