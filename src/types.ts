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
}
export interface Snapshot {
  assets: Asset[]
  groups: Group[]
  settings: Settings
}
export interface JobStatus {
  running: boolean
  done: number
  total: number
  message: string
  error: string | null
}
export interface ModelStatus {
  ready: boolean
  directory: string
  missing: string[]
  job: JobStatus
  import: JobStatus
}
export interface ImportReport {
  added: number
  duplicates: number
  errors: string[]
}
export type SearchMode = 'hybrid' | 'keyword' | 'semantic'
