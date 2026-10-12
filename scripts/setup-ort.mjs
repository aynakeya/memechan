import { execFileSync } from 'node:child_process'
import console from 'node:console'
import { createHash } from 'node:crypto'
import {
  closeSync,
  createReadStream,
  existsSync,
  mkdirSync,
  mkdtempSync,
  openSync,
  renameSync,
  rmSync,
} from 'node:fs'
import path from 'node:path'
import process from 'node:process'
import { parseArgs } from 'node:util'
import manifest from './onnxruntime.json' with { type: 'json' }

const { values } = parseArgs({
  options: {
    download: { type: 'boolean' },
    archive: { type: 'string' },
    target: { type: 'string' },
    help: { type: 'boolean' },
  },
})

if (values.help || (!values.download && !values.archive)) {
  console.log(`准备 ONNX Runtime ${manifest.version}（ort ${manifest.ortVersion}）
  pnpm setup:ort --download                  显式下载并校验
  pnpm setup:ort --archive /path/to/file      使用本地 .tar.lzma2，不联网
  可选 --target <Rust target triple>，默认使用 rustc 的 host。
需要 curl（仅下载）、xz 和 tar；Windows 可使用 Git Bash 提供这些工具。
目标目录默认是 .local/onnxruntime，也可通过 ORT_LIB_PATH 指定。`)
  process.exit(values.help ? 0 : 1)
}

let staging
try {
  if (values.download && values.archive) {
    throw new Error('--download 和 --archive 只能选择一个')
  }
  const target =
    values.target ??
    execFileSync('rustc', ['-vV'], { encoding: 'utf8' }).match(/^host: (.+)$/m)?.[1]
  const distribution = manifest.targets[target]
  if (!distribution) {
    throw new Error(`没有 ${target} 的预编译包。请自行准备兼容的静态库并设置 ORT_LIB_PATH。`)
  }
  const root = path.resolve(import.meta.dirname, '..')
  const destination = path.resolve(
    process.env.ORT_LIB_PATH || path.join(root, '.local/onnxruntime'),
  )
  if (existsSync(destination)) {
    throw new Error(`目标目录已存在：${destination}。如需重新准备，请先将该目录移走。`)
  }
  const archiveSource = values.archive && path.resolve(values.archive)
  console.log(`ONNX Runtime: ${manifest.version}\nTarget: ${target}`)
  console.log(`URL: ${distribution.url}\nSHA-256: ${distribution.sha256}`)
  console.log(`来源: ${archiveSource || '联网下载'}\n目标: ${destination}`)

  // Check tools before downloading. Compilation never invokes this script.
  for (const tool of values.download ? ['curl', 'xz', 'tar'] : ['xz', 'tar']) {
    execFileSync(tool, ['--version'], { stdio: 'ignore' })
  }
  mkdirSync(path.dirname(destination), { recursive: true })
  staging = mkdtempSync(path.join(path.dirname(destination), 'ort-setup-'))
  const archive = archiveSource || path.join(staging, 'runtime.tar.lzma2')
  if (values.download) {
    execFileSync(
      'curl',
      ['-4', '--fail', '--location', '--show-error', '--output', archive, distribution.url],
      {
        stdio: 'inherit',
      },
    )
  }
  const hash = createHash('sha256')
  for await (const chunk of createReadStream(archive)) hash.update(chunk)
  const actual = hash.digest('hex')
  if (actual !== distribution.sha256) {
    throw new Error(`SHA-256 不匹配：预期 ${distribution.sha256}，实际 ${actual}`)
  }

  // Pyke distributes raw LZMA2, not an .xz container. Verify before extracting.
  const tarFile = path.join(staging, 'runtime.tar')
  const output = openSync(tarFile, 'wx')
  try {
    execFileSync(
      'xz',
      ['--format=raw', '--lzma2=dict=64MiB', '--decompress', '--stdout', archive],
      {
        stdio: ['ignore', output, 'inherit'],
      },
    )
  } finally {
    closeSync(output)
  }
  const extracted = path.join(staging, 'lib')
  mkdirSync(extracted)
  // Relative paths avoid GNU tar treating a Windows drive letter as a remote host.
  execFileSync('tar', ['-xf', 'runtime.tar', '-C', 'lib'], {
    cwd: staging,
    stdio: 'inherit',
  })
  const library = target.includes('windows') ? 'onnxruntime.lib' : 'libonnxruntime.a'
  if (!existsSync(path.join(extracted, library))) {
    throw new Error(`压缩包缺少 ${library}`)
  }
  renameSync(extracted, destination)
  console.log(`已准备：${destination}\n后续编译从该目录静态链接，不会自动下载 ONNX Runtime。`)
} catch (error) {
  console.error(error.message)
  process.exitCode = 1
} finally {
  if (staging) rmSync(staging, { recursive: true, force: true })
}
