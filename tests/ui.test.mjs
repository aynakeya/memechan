import { test } from 'node:test'
import assert from 'node:assert/strict'
import process from 'node:process'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { URL } from 'node:url'
import { chromium } from 'playwright-core'

// This tests the UI's desktop command contract. Native file/clipboard/model behavior
// is covered separately; these fixtures never ship in the application.
test('library selection, description, copy size and failed search remain usable', async () => {
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
    assert.equal(await page.locator('.asset-card').count(), 6)
    await page
      .locator('.asset-card img')
      .first()
      .evaluate((img) => img.decode())
    await page.evaluate(() =>
      window.testEmit('tauri://drag-enter', {
        paths: ['/memes/开心.gif'],
        position: { x: 40, y: 40 },
      }),
    )
    await page.locator('.drop-overlay').waitFor()
    await page.evaluate(() => window.testEmit('tauri://drag-leave'))
    await page.locator('.drop-overlay').waitFor({ state: 'hidden' })
    await page.evaluate(() =>
      window.testEmit('tauri://drag-drop', {
        paths: ['/memes/开心.gif'],
        position: { x: 40, y: 40 },
      }),
    )
    await page.getByText('已添加 1 张表情', { exact: true }).waitFor()
    assert.ok(
      await page.evaluate(() =>
        window.testCalls.some(
          (call) => call.command === 'import_files' && call.args.paths[0] === '/memes/开心.gif',
        ),
      ),
    )
    await page.screenshot({ path: join(tmpdir(), 'memechan-library.png') })
    await page.locator('.asset-card').nth(1).click()
    await page.getByLabel('我的描述').fill('周一不想上班')
    await page.getByRole('button', { name: '保存描述', exact: true }).click()
    await page.getByRole('button', { name: '已保存', exact: true }).waitFor()
    await page.locator('.asset-card').nth(1).click({ button: 'right' })
    await page.getByRole('menuitem', { name: '最长边 256 px', exact: true }).focus()
    await page.keyboard.press('Enter')
    assert.ok(
      await page.evaluate(() =>
        window.testCalls.some(
          (call) => call.command === 'copy_asset' && call.args.id === '1' && call.args.size === 256,
        ),
      ),
    )
    await page.screenshot({ path: join(tmpdir(), 'memechan-details.png') })
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
    await page.evaluate(() => {
      window.testModel.ready = false
      window.testModel.missing = ['qwen3-embedding-0.6b.onnx', 'qwen3-tokenizer.json']
      window.testModelImportResult = false
    })
    await page.getByRole('button', { name: '偏好设置' }).click()
    await page.getByRole('tab', { name: '本地搜索模型', exact: true }).click()
    await page.getByText('还需导入：', { exact: false }).waitFor()
    await page.getByRole('button', { name: '导入本地模型', exact: true }).click()
    assert.equal(
      await page.getByRole('button', { name: '导入本地模型', exact: true }).isEnabled(),
      true,
    )
    assert.equal(await page.getByRole('button', { name: '继续处理待索引表情' }).count(), 0)
    await page.evaluate(() => {
      window.testModelImportResult = true
    })
    await page.getByRole('button', { name: '导入本地模型', exact: true }).click()
    await page.getByRole('button', { name: '继续处理待索引表情' }).waitFor()
    assert.equal(
      await page.evaluate(() => window.testCalls.some((call) => call.command === 'install_models')),
      false,
    )
    await page.screenshot({ path: join(tmpdir(), 'memechan-local-models.png') })
    await page.getByRole('button', { name: '关闭弹窗' }).click()
    await page.setViewportSize({ width: 940, height: 640 })
    await page.screenshot({ path: join(tmpdir(), 'memechan-small.png') })
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
    await page.getByText('已添加 3 张表情', { exact: true }).waitFor()
    assert.equal(await page.locator('.asset-card').count(), 9)
    assert.deepEqual(errors, [])
    assert.deepEqual(externalRequests, [])
  } finally {
    await browser.close()
  }
})
