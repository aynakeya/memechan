export interface Asset {
  id: string
  name: string
  path: string
  thumbnail: string
  format: string
  width: number
  height: number
  bytes: number
  animated: boolean
  description: string
  aiDescription: string
  aiState: string
  aiError: string
  aiModel: string
  ocrText: string
  ocrState: string
  indexState: string
  favorite: boolean
  createdAt: number
  groups: number[]
  score?: number
}
export interface Group {
  id: number
  name: string
  color: string
  count: number
}
export interface Settings {
  copySize: number
  allowUpscale: boolean
  autoIndex: boolean
  ocrEnabled: boolean
  embeddingModel: 'qwen3-0.6b' | 'qwen3-4b' | 'qwen3-8b'
}
export interface Snapshot {
  assets: Asset[]
  groups: Group[]
  settings: Settings
}
export interface JobStatus {
  running: boolean
  paused: boolean
  pending: number
  failed: number
  completed: number
  incomplete: number
  done: number
  total: number
  message: string
  error: string | null
}
export interface ModelStatus {
  ready: boolean
  ocrReady: boolean
  ocrEnabled: boolean
  directory: string
  missing: string[]
  job: JobStatus
  import: JobStatus
  ocr: JobStatus
  index: JobStatus
  vision: JobStatus
}
export interface VisionConfig {
  baseUrl: string
  model: string
  autoDescribe: boolean
  apiKey: string
}
export interface VisionStatus {
  config: VisionConfig
  active: boolean
}
export interface ImportReport {
  added: number
  duplicates: number
  errors: string[]
}
export type SearchMode = 'hybrid' | 'keyword' | 'semantic'
export type TaskKind = 'ocr' | 'index' | 'vision'
export type TaskAction = 'pause' | 'resume' | 'retry' | 'generate'
export type ClearKind = TaskKind
