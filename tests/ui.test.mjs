import { test } from 'node:test'
import assert from 'node:assert/strict'
import process from 'node:process'
import { URL } from 'node:url'
import { chromium } from 'playwright-core'

// This tests the UI's desktop command contract. Native file/clipboard/model behavior
// is covered separately; these fixtures never ship in the application.
function desktopTest(name, run) {
  return test(name, { timeout: 60000 }, async () => {
    const browser = await chromium.launch({
      headless: true,
      executablePath: process.env.PLAYWRIGHT_CHROMIUM_EXECUTABLE,
    })
    try {
      const page = await browser.newPage({ viewport: { width: 1320, height: 860 } })
      const errors = []
      const externalRequests = []
      page.on('pageerror', (error) => errors.push(error.message))
      page.on('request', (request) => {
        const url = new URL(request.url())
        if (
          url.protocol.startsWith('http') &&
          !['127.0.0.1', 'localhost', 'asset.localhost'].includes(url.hostname)
        )
          externalRequests.push(request.url())
      })
      await page.route('**/src/main.ts*', async (route) => {
        const response = await route.fetch()
        await route.fulfill({
          response,
          body: 'import "/tests/tauri-mock.mjs";\n' + (await response.text()),
        })
      })
      await page.route('http://asset.localhost/**', async (route) => {
        const dataUrl = decodeURIComponent(new URL(route.request().url()).pathname.slice(1))
        await route.fulfill({
          contentType: 'image/svg+xml',
          body: decodeURIComponent(dataUrl.slice(dataUrl.indexOf(',') + 1)),
        })
      })
      await page.goto(process.env.MEMECHAN_TEST_URL || 'http://127.0.0.1:1420/')
      await page.locator('.asset-card').first().waitFor()
      await run(page)
      assert.deepEqual(errors, [])
      assert.deepEqual(externalRequests, [])
    } finally {
      await browser.close()
    }
  })
}

desktopTest('编辑描述、复制尺寸、预览及搜索错误恢复', async (page) => {
  const asset = page.getByRole('article', { name: '正在认真摸鱼', exact: true })
  await asset.click()
  await page.getByLabel('我的描述').fill('周一不想上班')
  await page.getByRole('button', { name: '保存描述', exact: true }).click()
  await page.getByRole('button', { name: '已保存', exact: true }).waitFor()
  await page.evaluate(() => {
    window.testHoldSave = true
  })
  await page.getByRole('checkbox', { name: '打工日常', exact: true }).click()
  await page.waitForFunction(() => !!window.testFinishSave)
  await page.getByLabel('我的描述').fill('仍然想下班')
  await page.evaluate(() => {
    window.testFinishSave()
    window.testHoldSave = false
  })
  await page.getByRole('button', { name: '保存描述', exact: true }).waitFor()
  assert.equal(
    await page
      .getByRole('checkbox', { name: '打工日常', exact: true })
      .getAttribute('aria-checked'),
    'false',
  )
  await page.getByRole('button', { name: '保存描述', exact: true }).click()
  await page.getByRole('button', { name: '已保存', exact: true }).waitFor()
  await asset.click({ button: 'right' })
  await page.getByRole('menuitem', { name: '最长边 256 px', exact: true }).focus()
  await page.keyboard.press('Enter')
  await page.waitForFunction(() =>
    window.testCalls.some(
      (call) => call.command === 'copy_asset' && call.args.id === '1' && call.args.size === 256,
    ),
  )
  await page.getByRole('button', { name: '关闭详情' }).click()
  await page.locator('.asset-card').first().focus()
  await page.keyboard.press('Space')
  await page.getByRole('dialog', { name: '原图预览', exact: true }).waitFor()
  await page.keyboard.press('Escape')
  await page.getByRole('dialog', { name: '原图预览', exact: true }).waitFor({ state: 'hidden' })
  await page.getByRole('button', { name: '只看动图' }).click()
  assert.equal(await page.locator('.asset-card').count(), 1)
  await page.getByRole('button', { name: '只看动图' }).click()
  await page.getByRole('button', { name: '偏好设置' }).click()
  await page.getByLabel('默认复制最长边').fill('320')
  await page.getByRole('button', { name: '保存', exact: true }).click()
  await page.getByRole('button', { name: '关闭弹窗' }).click()
  await page.locator('.asset-card').first().dblclick()
  assert.ok(
    await page.evaluate(() =>
      window.testCalls.some(
        (call) => call.command === 'save_settings' && call.args.settings.copySize === 320,
      ),
    ),
  )
  await page.keyboard.press('Control+k')
  assert.equal(
    await page.getByLabel('搜索表情').evaluate((input) => document.activeElement === input),
    true,
  )
  await page.getByLabel('搜索表情').fill('不想上班')
  await page.getByRole('alert').filter({ hasText: '本地模型未就绪' }).waitFor()
  await page.getByRole('button', { name: '清空搜索' }).click()
  await page.waitForFunction(() => document.querySelectorAll('.asset-card').length === 6)
})

desktopTest('设置草稿、本地模型与图片理解配置', async (page) => {
  await page.evaluate(() => {
    window.testModel.ready = false
    window.testModel.missing = ['qwen3-embedding-0.6b.onnx', 'qwen3-tokenizer.json']
    window.testModelImportResult = false
  })
  await page.getByRole('button', { name: '偏好设置' }).click()
  await page.getByLabel('默认复制最长边').fill('640')
  const snapshots = await page.evaluate(
    () => window.testCalls.filter((call) => call.command === 'snapshot').length,
  )
  await page.evaluate(() => {
    window.testModel.job.done++
  })
  await page.waitForFunction(
    (count) => window.testCalls.filter((call) => call.command === 'snapshot').length > count,
    snapshots,
  )
  assert.equal(await page.getByLabel('默认复制最长边').inputValue(), '640')
  await page.getByRole('tab', { name: 'Embedding', exact: true }).click()
  await page.getByText('还需导入：', { exact: false }).waitFor()
  await page.getByRole('button', { name: '导入本地模型', exact: true }).click()
  assert.equal(
    await page.getByRole('button', { name: '导入本地模型', exact: true }).isEnabled(),
    true,
  )
  await page.evaluate(() => {
    window.testModelImportResult = true
  })
  await page.getByRole('button', { name: '导入本地模型', exact: true }).click()
  await page.waitForFunction(() => window.testModel.ready)
  await page.getByRole('tab', { name: '图片理解 API', exact: true }).click()
  assert.equal(
    await page.getByRole('button', { name: '测试图片理解', exact: true }).isEnabled(),
    false,
  )
  await page.getByLabel('API Key', { exact: true }).fill('ui-test-key')
  await page.getByRole('button', { name: '保存 API 配置', exact: true }).click()
  await page.getByText('API 配置已保存', { exact: true }).waitFor()
  await page.getByRole('button', { name: '关闭弹窗', exact: true }).click()
  await page.getByRole('button', { name: '偏好设置', exact: true }).click()
  await page.getByRole('tab', { name: '图片理解 API', exact: true }).click()
  const key = page.getByLabel('API Key', { exact: true })
  assert.equal(await key.inputValue(), 'ui-test-key')
  assert.equal(await key.getAttribute('type'), 'password')
  await page.getByRole('button', { name: '显示 API Key', exact: true }).click()
  assert.equal(await key.getAttribute('type'), 'text')
  await page.getByRole('button', { name: '隐藏 API Key', exact: true }).click()
  await page.getByLabel('API 地址', { exact: true }).fill('http://localhost:8080/v1')
  assert.equal(await key.inputValue(), '')
  for (const value of ['ui-updated-key', '']) {
    await key.fill(value)
    await page.getByRole('button', { name: '保存 API 配置', exact: true }).click()
    await page.waitForFunction(
      (expected) =>
        window.testCalls.some(
          (call) => call.command === 'save_vision_config' && call.args.config.apiKey === expected,
        ),
      value,
    )
    await page.getByRole('button', { name: '关闭弹窗', exact: true }).click()
    await page.getByRole('button', { name: '偏好设置', exact: true }).click()
    await page.getByRole('tab', { name: '图片理解 API', exact: true }).click()
    assert.equal(await key.inputValue(), value)
    assert.equal(await key.getAttribute('type'), 'password')
  }
  await page.getByRole('button', { name: '测试图片理解', exact: true }).click()
  await page.getByRole('status').filter({ hasText: '左侧红色方块' }).waitFor()
  await page.getByRole('button', { name: '关闭弹窗', exact: true }).click()
  await page.getByRole('button', { name: '任务管理', exact: true }).click()
  const task = page.getByRole('region', { name: '图片理解 API', exact: true })
  await task.getByRole('button', { name: '生成缺失描述', exact: true }).click()
  await task.getByRole('button', { name: '暂停', exact: true }).click()
  await task.getByText('已暂停', { exact: true }).waitFor()
  await page.getByRole('button', { name: '关闭任务管理' }).click()
  await page.getByRole('article', { name: '正在认真摸鱼', exact: true }).click()
  await page.getByRole('button', { name: '通过 API 生成描述', exact: true }).click()
  await page.getByText('女孩举手敬礼，表示收到。', { exact: true }).waitFor()
  assert.equal(await page.getByLabel('我的描述').inputValue(), '假装忙碌，其实想下班')
  await page.getByRole('button', { name: '清除这张图片的描述', exact: true }).click()
  await page.getByRole('button', { name: '取消', exact: true }).click()
  await page.getByText('女孩举手敬礼，表示收到。', { exact: true }).waitFor()
  await page.getByRole('button', { name: '清除这张图片的描述', exact: true }).click()
  await page.getByRole('button', { name: '确认清除', exact: true }).click()
  await page.getByText('女孩举手敬礼，表示收到。', { exact: true }).waitFor({ state: 'hidden' })
  await page.getByRole('button', { name: '清除这张图片的 OCR 文字', exact: true }).click()
  await page.getByRole('button', { name: '确认清除', exact: true }).click()
  await page.getByText('文字已清除，可重新识别。', { exact: true }).waitFor()
  assert.equal(await page.getByLabel('我的描述').inputValue(), '假装忙碌，其实想下班')
})

desktopTest('停用 OCR、独立控制任务与清除整个资料库的文字', async (page) => {
  await page.evaluate(() => {
    window.testModel.index.message = 'last-completed-meme.png'
  })
  await page.getByRole('button', { name: '偏好设置', exact: true }).click()
  await page.getByRole('tab', { name: 'OCR', exact: true }).click()
  await page.getByRole('switch', { name: '启用本地 OCR', exact: true }).click()
  await page.waitForFunction(() => !window.testModel.ocrEnabled)
  await page.getByRole('button', { name: '关闭弹窗', exact: true }).click()
  await page.getByRole('button', { name: '任务管理', exact: true }).click()
  const ocr = page.getByRole('region', { name: '本地 OCR', exact: true })
  await ocr.getByText('已停用', { exact: true }).waitFor()
  await ocr.getByText('已识别 6', { exact: true }).waitFor()
  await ocr.getByText('未识别 0', { exact: true }).waitFor()
  const vision = page.getByRole('region', { name: '图片理解 API', exact: true })
  await vision.getByText('已生成 0', { exact: true }).waitFor()
  await vision.getByText('未生成 6', { exact: true }).waitFor()
  const index = page.getByRole('region', { name: '文字向量索引', exact: true })
  await index.getByText('已索引 6', { exact: true }).waitFor()
  assert.equal(await index.getByText('last-completed-meme.png', { exact: true }).count(), 0)
  assert.equal(await index.getByRole('button', { name: '继续', exact: true }).count(), 0)
  const progress = index
    .getByRole('group', { name: '文字向量索引进度', exact: true })
    .getByRole('progressbar')
  assert.equal(
    await progress.getAttribute('aria-valuenow'),
    await progress.getAttribute('aria-valuemax'),
  )
  await index.getByRole('button', { name: '清除全部文字索引', exact: true }).click()
  await page.getByRole('button', { name: '确认清除', exact: true }).click()
  await index.getByText('已索引 0', { exact: true }).waitFor()
  await index.getByText('未索引 6', { exact: true }).waitFor()
  await ocr.getByText('已识别 6', { exact: true }).waitFor()
  assert.equal(await progress.getAttribute('aria-valuenow'), '0')
  await index.getByRole('button', { name: '建立缺失索引', exact: true }).click()
  await index.getByRole('button', { name: '暂停', exact: true }).click()
  await page.evaluate(() =>
    Object.assign(window.testModel.index, { pending: 2, failed: 1, error: '测试索引失败' }),
  )
  await index.getByRole('button', { name: '重试失败项', exact: true }).click()
  await index.getByRole('button', { name: '暂停', exact: true }).click()
  await index.getByRole('button', { name: '继续', exact: true }).click()
  await index.getByRole('button', { name: '暂停', exact: true }).waitFor()
  await ocr.getByRole('button', { name: '清除全部 OCR 文字', exact: true }).click()
  await page.getByRole('button', { name: '确认清除', exact: true }).click()
  await ocr.getByText('已识别 0', { exact: true }).waitFor()
  await ocr.getByText('未识别 6', { exact: true }).waitFor()
  await index.getByText('未索引 6', { exact: true }).waitFor()
  await vision.getByRole('button', { name: '清除全部图片描述', exact: true }).click()
  await page.getByRole('button', { name: '确认清除', exact: true }).click()
  await page.getByRole('button', { name: '关闭任务管理', exact: true }).click()
  for (const name of ['今天也要开心', '正在认真摸鱼']) {
    await page.getByRole('article', { name, exact: true }).click()
    await page.getByText('文字已清除，可重新识别。', { exact: true }).waitFor()
  }
  assert.equal(await page.getByLabel('我的描述').inputValue(), '假装忙碌，其实想下班')
  assert.deepEqual(
    await page.evaluate(() =>
      window.testCalls.filter((call) => call.command === 'clear_text').map((call) => call.args),
    ),
    [
      { kind: 'index', id: null },
      { kind: 'ocr', id: null },
      { kind: 'vision', id: null },
    ],
  )
})

desktopTest('批量导入逐张可见且不阻塞浏览', async (page) => {
  await page.evaluate(() =>
    window.testEmit('tauri://drag-enter', {
      paths: ['/memes/开心.gif'],
      position: { x: 40, y: 40 },
    }),
  )
  await page.locator('.drop-overlay').waitFor()
  await page.evaluate(() => window.testEmit('tauri://drag-leave'))
  await page.locator('.drop-overlay').waitFor({ state: 'hidden' })
  await page.setViewportSize({ width: 940, height: 640 })
  await page.evaluate(() =>
    window.testEmit('tauri://drag-drop', {
      paths: ['/memes/new-1.png', '/memes/new-2.png', '/memes/new-3.png'],
      position: { x: 40, y: 40 },
    }),
  )
  await page
    .getByRole('status')
    .filter({ hasText: /导入 \d+ \/ 3/ })
    .waitFor()
  await page.locator('.asset-card').nth(6).waitFor()
  const duringImport = await page.locator('.asset-card').count()
  assert.ok(
    duringImport > 6 && duringImport < 9,
    'finished items appear before the whole import completes',
  )
  await page.locator('.asset-card').first().click()
  await page.getByLabel('我的描述').waitFor()
  await page.evaluate(() => window.testResumeImport())
  await page.getByText('已添加 3 张表情', { exact: true }).waitFor()
  assert.equal(await page.locator('.asset-card').count(), 9)
})
