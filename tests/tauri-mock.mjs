import { mockIPC, mockWindows, mockConvertFileSrc } from '@tauri-apps/api/mocks'
import { emit } from '@tauri-apps/api/event'

window.isTauri = true
mockWindows('main')
mockConvertFileSrc('windows')
window.testEmit = emit

const names = ['今天也要开心', '正在认真摸鱼', '谢谢老板', '这合理吗', '让我静静', '好耶！']
const faces = ['◡', '﹏', 'ᴗ', '○', '︿', '▽']
const assets = names.map((name, i) => {
  const svg = `<svg xmlns="http://www.w3.org/2000/svg" width="256" height="256"><circle cx="128" cy="108" r="65" fill="${['#e7cd85', '#afc6ab', '#e3b9a2'][i % 3]}"/><circle cx="105" cy="96" r="5" fill="#45533f"/><circle cx="150" cy="96" r="5" fill="#45533f"/><text x="128" y="144" text-anchor="middle" font-size="54" fill="#45533f">${faces[i]}</text><text x="128" y="220" text-anchor="middle" font-size="20" fill="#45533f">${name}</text></svg>`
  const path = `data:image/svg+xml,${encodeURIComponent(svg)}`
  return {
    id: String(i),
    name,
    path,
    thumbnail: path,
    format: i === 1 ? 'gif' : 'png',
    width: 256,
    height: 256,
    bytes: 20480,
    animated: i === 1,
    description: i === 1 ? '假装忙碌，其实想下班' : '',
    aiDescription: '',
    aiState: 'idle',
    aiError: '',
    aiModel: '',
    ocrText: name,
    ocrState: 'ready',
    indexState: 'ready',
    favorite: i === 0,
    createdAt: Date.now(),
    groups: i < 3 ? [1] : [],
    score: null,
  }
})
const settings = { copySize: 0, allowUpscale: false, autoIndex: true, ocrEnabled: true }
const vision = {
  config: {
    baseUrl: 'https://api.deepseek.com',
    model: 'deepseek-v4-flash',
    autoDescribe: false,
    apiKey: '',
  },
  active: false,
}
window.testCalls = []
window.testModelImportResult = true
const job = () => ({
  running: false,
  paused: false,
  pending: 0,
  failed: 0,
  completed: 0,
  incomplete: 0,
  done: 0,
  total: 0,
  message: '',
  error: null,
})
window.testModel = {
  ocrReady: true,
  ocrEnabled: true,
  ready: true,
  directory: '/local/models',
  missing: [],
  job: job(),
  import: job(),
  vision: job(),
  ocr: job(),
  index: job(),
}

mockIPC(
  async (command, args) => {
    window.testCalls.push({ command, args })
    if (command === 'snapshot')
      return JSON.parse(
        JSON.stringify({
          assets,
          groups: [{ id: 1, name: '打工日常', color: '#88966d', count: 3 }],
          settings,
        }),
      )
    if (command === 'model_status') {
      const status = window.structuredClone(window.testModel)
      for (const [kind, field] of [
        ['ocr', 'ocrState'],
        ['vision', 'aiState'],
        ['index', 'indexState'],
      ]) {
        status[kind].completed = assets.filter((asset) => asset[field] === 'ready').length
        status[kind].incomplete = assets.length - status[kind].completed
      }
      return status
    }
    if (command === 'vision_status') return JSON.parse(JSON.stringify(vision))
    if (command === 'save_vision_config') {
      vision.config = { ...args.config }
      vision.active = true
      return
    }
    if (command === 'test_vision')
      return { model: 'test-vision', text: '左侧红色方块，右侧蓝色圆形。' }
    if (command === 'control_task') {
      const task = window.testModel[args.kind]
      if (args.action === 'pause') {
        Object.assign(task, { running: false, paused: true, message: '任务已暂停' })
        return
      }
      Object.assign(task, { running: true, paused: false, done: 0, error: null, failed: 0 })
      if (args.kind === 'index' && args.action === 'generate') {
        for (const asset of assets) {
          if (['idle', 'error'].includes(asset.indexState)) asset.indexState = 'pending'
        }
        task.pending = assets.filter((asset) => asset.indexState === 'pending').length
      }
      if (args.kind === 'vision' && args.id) {
        const asset = assets.find((asset) => asset.id === args.id)
        asset.aiState = 'pending'
        task.pending = 1
        window.setTimeout(() => {
          if (asset.aiState === 'pending')
            Object.assign(asset, {
              aiDescription: '女孩举手敬礼，表示收到。',
              aiState: 'ready',
              aiModel: 'test-vision',
            })
          Object.assign(task, { running: false, done: 1, pending: 0, message: '描述生成完成' })
        }, 500)
      }
      return
    }
    if (command === 'clear_text') {
      const selected = assets.filter((asset) => !args.id || asset.id === args.id)
      for (const asset of selected) {
        if (args.kind === 'ocr') Object.assign(asset, { ocrText: '', ocrState: 'idle' })
        else if (args.kind === 'vision')
          Object.assign(asset, { aiDescription: '', aiState: 'idle', aiModel: '', aiError: '' })
        if (args.kind === 'index') asset.indexState = 'idle'
        else if (asset.indexState !== 'idle') asset.indexState = 'pending'
      }
      if (args.kind === 'index')
        Object.assign(window.testModel.index, { pending: 0, failed: 0, error: null })
      return selected.length
    }
    if (command === 'import_models') {
      if (window.testModelImportResult) {
        window.testModel.ready = true
        window.testModel.missing = []
        window.testModel.job.message = '整理完成'
      }
      return window.testModelImportResult
    }
    if (command === 'save_asset') {
      if (window.testHoldSave)
        await new Promise((resolve) => {
          window.testFinishSave = resolve
        })
      return Object.assign(
        assets.find((a) => a.id === args.id),
        { description: args.description, favorite: args.favorite, groups: args.groups },
      )
    }
    if (command === 'copy_asset') return '已复制图片'
    if (command === 'save_settings') {
      window.testModel.ocrEnabled = args.settings.ocrEnabled
      return Object.assign(settings, args.settings)
    }
    if (command === 'search') throw new Error('测试：本地模型未就绪')
    if (command === 'import_files') {
      if (args.paths.length > 1) {
        Object.assign(window.testModel.import, { running: true, done: 0, total: args.paths.length })
        for (const path of args.paths) {
          assets.push({ ...assets[0], id: path, name: path, groups: [] })
          window.testModel.import.done++
          if (window.testModel.import.done === 1)
            await new Promise((resolve) => {
              window.testResumeImport = resolve
            })
        }
        window.testModel.import.running = false
      }
      return { added: args.paths.length, duplicates: 0, errors: [] }
    }
    throw new Error('未模拟的桌面命令：' + command)
  },
  { shouldMockEvents: true },
)
