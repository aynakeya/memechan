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

desktopTest('详情开关、编辑描述、复制尺寸、预览及搜索错误恢复', async (page) => {
  const asset = page.getByRole('article', { name: '正在认真摸鱼', exact: true })
  const details = page.getByRole('button', { name: '详情', exact: true })
  assert.equal(await details.isDisabled(), true)
  await asset.click()
  assert.equal(await page.locator('.details-panel').count(), 0)
  await details.click()
  assert.equal(await details.getAttribute('aria-pressed'), 'true')
  await page.getByRole('article', { name: '谢谢老板', exact: true }).click()
  await page.locator('.details-panel').getByRole('heading', { name: '谢谢老板' }).waitFor()
  await asset.click()
  await page.getByLabel('我的描述').fill('周一不想上班')
  await details.click()
  await page.locator('.details-panel').waitFor({ state: 'hidden' })
  assert.equal(
    await page.locator('.asset-card.selected').getAttribute('aria-label'),
    '正在认真摸鱼',
  )
  assert.equal(await details.getAttribute('aria-pressed'), 'false')
  await details.click()
  assert.equal(await page.getByLabel('我的描述').inputValue(), '周一不想上班')
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
  assert.equal(
    await page.locator('.asset-card.selected').getAttribute('aria-label'),
    '正在认真摸鱼',
  )
  await page.locator('.asset-card').first().focus()
  assert.equal(await page.locator('.details-panel').count(), 0)
  await page.keyboard.press('Space')
  await page.getByRole('dialog', { name: '原图预览', exact: true }).waitFor()
  await page.keyboard.press('Escape')
  await page.getByRole('dialog', { name: '原图预览', exact: true }).waitFor({ state: 'hidden' })
  assert.equal(await page.locator('.details-panel').count(), 0)
  await page.getByRole('button', { name: '只看动图' }).click()
  assert.equal(await page.locator('.asset-card').count(), 1)
  await page.getByRole('button', { name: '只看动图' }).click()
  await page.getByRole('button', { name: '偏好设置' }).click()
  await page.getByLabel('默认复制最长边').fill('320')
  await page.getByRole('button', { name: '保存', exact: true }).click()
  await page.getByRole('button', { name: '关闭弹窗' }).click()
  await page.locator('.asset-card').first().dblclick()
  await page.waitForFunction(() =>
    window.testCalls.some((call) => call.command === 'copy_asset' && call.args.size === null),
  )
  assert.equal(await page.locator('.details-panel').count(), 0)
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
  const search = page.getByLabel('搜索表情')
  await search.fill('bu')
  await search.evaluate((input) => {
    input.dispatchEvent(new window.CompositionEvent('compositionstart', { bubbles: true }))
    input.value = 'bu xiang'
    input.dispatchEvent(new window.InputEvent('input', { bubbles: true, isComposing: true }))
    // A task refresh must not trigger a search for unfinished composition either.
    window.testModel.job.done++
    window.testModel.libraryRevision++
  })
  await page.waitForTimeout(2000)
  assert.deepEqual(
    await page.evaluate(() => window.testCalls.filter((call) => call.command === 'search')),
    [],
  )
  await search.evaluate((input) => {
    input.value = '不想上班'
    input.dispatchEvent(
      new window.CompositionEvent('compositionend', { bubbles: true, data: input.value }),
    )
    input.dispatchEvent(new window.InputEvent('input', { bubbles: true, isComposing: false }))
  })
  await page.getByRole('alert').filter({ hasText: '本地模型未就绪' }).waitFor()
  assert.deepEqual(
    await page.evaluate(() =>
      window.testCalls.filter((call) => call.command === 'search').map((call) => call.args.query),
    ),
    ['不想上班'],
  )
  await page.getByRole('button', { name: '清空搜索' }).click()
  await page.waitForFunction(() => document.querySelectorAll('.asset-card').length === 6)
})

desktopTest('批量管理：修饰键连选、分组增减、删除确认和失败重试', async (page) => {
  const cards = page.locator('.asset-card')
  const names = await cards.evaluateAll((items) =>
    items.map((item) => item.getAttribute('aria-label')),
  )
  await cards.nth(0).click()
  await cards.nth(2).click({ modifiers: ['Control'] })
  assert.equal(await page.locator('.asset-card.selected').count(), 2)
  await cards.nth(4).click({ modifiers: ['Shift'] })
  assert.deepEqual(
    await page
      .locator('.asset-card.selected')
      .evaluateAll((items) => items.map((item) => item.getAttribute('aria-label'))),
    names.slice(2, 5),
  )
  await cards.nth(5).click({ modifiers: ['Meta'] })
  assert.equal(await page.locator('.asset-card.selected').count(), 4)
  await cards.nth(5).click({ modifiers: ['Meta'] })
  assert.equal(await page.locator('.asset-card.selected').count(), 3)

  const before = await page.evaluate(() => window.structuredClone(window.testAssets))
  await page.getByRole('button', { name: '批量分组', exact: true }).click()
  const groupDialog = page.getByRole('dialog', { name: '分组管理 · 3 张', exact: true })
  await page.evaluate(() => {
    window.testBatchError = '测试：分组保存失败'
  })
  await groupDialog.getByRole('button', { name: '应用', exact: true }).click()
  await page.getByText('测试：分组保存失败', { exact: true }).waitFor()
  assert.equal(await groupDialog.isVisible(), true)
  await page.evaluate(() => {
    window.testBatchError = ''
  })
  await groupDialog.getByRole('button', { name: '应用', exact: true }).click()
  await groupDialog.waitFor({ state: 'hidden' })
  const ids = await page.evaluate(
    () => window.testCalls.findLast((call) => call.command === 'set_assets_group').args.ids,
  )
  const grouped = await page.evaluate(() => window.structuredClone(window.testAssets))
  for (let i = 0; i < grouped.length; i++) {
    assert.deepEqual(grouped[i], {
      ...before[i],
      groups: ids.includes(before[i].id) ? [1] : before[i].groups,
    })
  }
  await page.getByRole('button', { name: '批量分组', exact: true }).click()
  await page.getByRole('combobox', { name: '分组操作', exact: true }).click()
  await page.getByRole('option', { name: '移出分组', exact: true }).click()
  await groupDialog.getByRole('button', { name: '应用', exact: true }).click()
  await groupDialog.waitFor({ state: 'hidden' })
  assert.equal(
    await page.evaluate(
      (selected) =>
        window.testAssets
          .filter((asset) => selected.includes(asset.id))
          .every((asset) => asset.groups.length === 0),
      ids,
    ),
    true,
  )

  await page.getByRole('button', { name: '删除所选', exact: true }).click()
  const deleteDialog = page.getByRole('dialog', { name: '删除选中的 3 张表情？', exact: true })
  await deleteDialog.getByRole('button', { name: '取消', exact: true }).click()
  assert.equal(
    await page.evaluate(
      () => window.testCalls.filter((call) => call.command === 'delete_assets').length,
    ),
    0,
  )
  await page.getByRole('button', { name: '删除所选', exact: true }).click()
  await deleteDialog.getByRole('button', { name: '确认删除', exact: true }).click()
  await deleteDialog.waitFor({ state: 'hidden' })
  await page.waitForFunction(() => document.querySelectorAll('.asset-card').length === 3)
  assert.equal(await page.locator('.asset-card.selected').count(), 0)
  assert.deepEqual(
    await page.evaluate(() => window.testAssets.map((asset) => asset.id)),
    before.filter((asset) => !ids.includes(asset.id)).map((asset) => asset.id),
  )

  await page.setViewportSize({ width: 600, height: 650 })
  await page.getByRole('button', { name: '多选', exact: true }).click()
  await cards.first().getByRole('checkbox').click()
  assert.equal(await page.locator('.asset-card.selected').count(), 1)
  await page.getByRole('button', { name: '全选当前结果', exact: true }).click()
  assert.equal(await page.locator('.asset-card.selected').count(), 3)
  assert.equal(
    await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth),
    true,
  )
  await page.getByRole('button', { name: '只看动图', exact: true }).click()
  assert.equal(await page.locator('.asset-card.selected').count(), 0)
  await page.getByRole('button', { name: '全选当前结果', exact: true }).click()
  assert.equal(await page.locator('.asset-card.selected').count(), 1)
  const search = page.getByLabel('搜索表情')
  await search.fill('typing')
  await search.press('Control+a')
  assert.equal(await search.evaluate((input) => input.selectionEnd - input.selectionStart), 6)
  await search.press('Delete')
  assert.equal(await search.inputValue(), '')
  assert.equal(await page.getByRole('dialog').count(), 0)
})

desktopTest('批量管理：虚拟列表跨屏连选及当前搜索结果全选', async (page) => {
  await page.evaluate(() => {
    const asset = window.testAssets[0]
    window.testAssets.splice(
      0,
      window.testAssets.length,
      ...Array.from({ length: 240 }, (_, index) => ({
        ...asset,
        id: `bulk-${index}`,
        name: `bulk-${index}`,
        createdAt: index,
      })),
    )
    window.testModel.libraryRevision++
  })
  const main = page.locator('.main-content')
  await page.getByRole('article', { name: 'bulk-239', exact: true }).waitFor()
  await page.getByRole('article', { name: 'bulk-239', exact: true }).click()
  await main.evaluate((element) => {
    element.scrollTop = element.scrollHeight
  })
  await page.getByRole('article', { name: 'bulk-0', exact: true }).click({ modifiers: ['Shift'] })
  await page.getByRole('status').filter({ hasText: '已选 240 张' }).waitFor()
  assert.ok((await page.locator('.asset-card').count()) < 60)
  await page.getByRole('button', { name: '批量分组', exact: true }).click()
  await page
    .getByRole('dialog', { name: '分组管理 · 240 张', exact: true })
    .getByRole('button', { name: '应用', exact: true })
    .click()
  await page.waitForFunction(() =>
    window.testCalls.some(
      (call) => call.command === 'set_assets_group' && call.args.ids.length === 240,
    ),
  )
  await page.getByRole('dialog').waitFor({ state: 'hidden' })
  await page.evaluate(() => {
    window.testHoldSearch = true
  })
  await main.evaluate((element) => {
    element.scrollTop = 0
  })
  await page.getByLabel('搜索表情').fill('批量检索')
  await page.waitForFunction(() => window.testPendingSearches.has('批量检索'))
  await page.evaluate(() =>
    window.testPendingSearches.get('批量检索')([window.testAssets[1], window.testAssets[9]]),
  )
  await page.waitForFunction(() => document.querySelectorAll('.asset-card').length === 2)
  await page.locator('.asset-card').first().click()
  await page.keyboard.press('Control+a')
  await page.getByRole('status').filter({ hasText: '已选 2 张' }).waitFor()
  await page.keyboard.press('Delete')
  await page
    .getByRole('dialog', { name: '删除选中的 2 张表情？', exact: true })
    .getByRole('button', { name: '确认删除', exact: true })
    .click()
  await page.getByRole('dialog').waitFor({ state: 'hidden' })
  assert.equal(await page.locator('.asset-card').count(), 0)
  assert.equal(await page.evaluate(() => window.testAssets.length), 238)
})

desktopTest('输入与组词立即取消旧搜索，后台刷新不重复推理', async (page) => {
  await page.evaluate(() => {
    window.testHoldSearch = true
  })
  const search = page.getByLabel('搜索表情')
  await search.fill('旧查询')
  await page.waitForFunction(() => window.testPendingSearches.has('旧查询'))
  const oldGeneration = await page.evaluate(() => window.testSearchGeneration)
  await search.evaluate((input) => {
    input.dispatchEvent(new window.CompositionEvent('compositionstart', { bubbles: true }))
    input.value = 'jing li'
    input.dispatchEvent(new window.InputEvent('input', { bubbles: true, isComposing: true }))
  })
  await page.waitForFunction((old) => window.testSearchGeneration > old, oldGeneration)
  await page.evaluate(() => window.testPendingSearches.get('旧查询')([window.testAssets[0]]))
  await page.waitForTimeout(400)
  assert.equal(await page.locator('.asset-card').count(), 6)
  assert.equal(
    await page.evaluate(() => window.testCalls.filter((call) => call.command === 'search').length),
    1,
  )
  await search.evaluate((input) => {
    input.value = '敬礼'
    input.dispatchEvent(
      new window.CompositionEvent('compositionend', { bubbles: true, data: input.value }),
    )
    input.dispatchEvent(new window.InputEvent('input', { bubbles: true }))
  })
  await page.waitForFunction(() => window.testPendingSearches.has('敬礼'))
  const snapshots = await page.evaluate(
    () => window.testCalls.filter((call) => call.command === 'snapshot').length,
  )
  await page.evaluate(() => {
    window.testModel.libraryRevision++
  })
  await page.waitForFunction(
    (count) => window.testCalls.filter((call) => call.command === 'snapshot').length > count,
    snapshots,
  )
  await page.waitForTimeout(400)
  assert.equal(
    await page.evaluate(() => window.testCalls.filter((call) => call.command === 'search').length),
    2,
  )
  await page.evaluate(() => window.testPendingSearches.get('敬礼')([window.testAssets[2]]))
  await page.waitForFunction(() => document.querySelectorAll('.asset-card').length === 1)
  assert.equal(await page.locator('.asset-card').getAttribute('aria-label'), '谢谢老板')

  await search.fill('新查询')
  await page.waitForFunction(() => window.testPendingSearches.has('新查询'))
  const generation = await page.evaluate(() => window.testSearchGeneration)
  await page.getByRole('button', { name: '清空搜索' }).click()
  await page.waitForFunction((old) => window.testSearchGeneration > old, generation)
  await page.evaluate(() => window.testPendingSearches.get('新查询')([window.testAssets[0]]))
  await page.waitForFunction(() => document.querySelectorAll('.asset-card').length === 6)
  assert.equal(await page.getByRole('alert').count(), 0)
})

desktopTest('大资料库虚拟滚动、尺寸切换和后台刷新', async (page) => {
  await page.waitForFunction(() => window.testCalls.some((call) => call.command === 'model_status'))
  const snapshots = await page.evaluate(
    () => window.testCalls.filter((call) => call.command === 'snapshot').length,
  )
  await page.evaluate(() => {
    window.testModel.job.running = true
    window.testModel.job.done++
  })
  await page.waitForTimeout(1800)
  assert.equal(
    await page.evaluate(
      () => window.testCalls.filter((call) => call.command === 'snapshot').length,
    ),
    snapshots,
  )
  await page.evaluate(() => {
    const template = window.testAssets[0]
    const assets = Array.from({ length: 2000 }, (_, i) => ({
      ...template,
      id: `large-${i}`,
      name: `大量表情 ${i}`,
      createdAt: 2000 - i,
      animated: i === 1999,
    }))
    window.testAssets.splice(0, window.testAssets.length, ...assets)
    window.testModel.libraryRevision++
    window.testModel.job.running = false
  })
  const first = page.getByRole('article', { name: '大量表情 0', exact: true })
  const last = page.getByRole('article', { name: '大量表情 1999', exact: true })
  await first.waitFor()
  assert.ok((await page.locator('.asset-card').count()) < 100)
  await page.locator('main').evaluate((main) => {
    main.scrollTop = main.scrollHeight
  })
  await last.waitFor()
  assert.equal(await first.count(), 0)
  await last.dblclick()
  await page.waitForFunction(() =>
    window.testCalls.some((call) => call.command === 'copy_asset' && call.args.id === 'large-1999'),
  )
  await last.click({ button: 'right' })
  await page.locator('main').evaluate((main) => {
    main.scrollTop = 0
  })
  await page.locator('.asset-card[aria-label="大量表情 0"]').waitFor()
  // A scrolled-away card stays mounted until its open menu closes.
  await page.getByRole('menuitem', { name: '最长边 256 px', exact: true }).focus()
  await page.keyboard.press('Enter')
  await page.waitForFunction(() =>
    window.testCalls.some(
      (call) =>
        call.command === 'copy_asset' && call.args.id === 'large-1999' && call.args.size === 256,
    ),
  )
  await page.locator('main').evaluate((main) => {
    main.scrollTop = 0
  })
  await last.waitFor({ state: 'detached' })
  await page.getByRole('button', { name: '切换为紧凑网格' }).click()
  for (const width of [640, 1550]) {
    await page.setViewportSize({ width, height: 600 })
    await page.locator('main').evaluate((main) => {
      main.scrollTop = main.scrollHeight
    })
    await last.waitFor()
    assert.ok((await page.locator('.asset-card').count()) < 100)
    const layout = await page.locator('.asset-grid').evaluate((grid) => {
      const cards = [...grid.querySelectorAll('.asset-card')].map((card) =>
        card.getBoundingClientRect(),
      )
      return {
        overlap: cards.some((card, i) =>
          cards
            .slice(i + 1)
            .some(
              (other) =>
                card.left < other.right - 1 &&
                card.right > other.left + 1 &&
                card.top < other.bottom - 1 &&
                card.bottom > other.top + 1,
            ),
        ),
        overflow:
          document.querySelector('main').scrollWidth > document.querySelector('main').clientWidth,
      }
    })
    assert.deepEqual(layout, { overlap: false, overflow: false })
  }
  await page.getByRole('button', { name: '只看动图' }).click()
  await last.waitFor()
  assert.equal(await page.locator('.asset-card').count(), 1)
  await page.getByRole('button', { name: '只看动图' }).click()
  await first.waitFor()
})

desktopTest('窗口切换保留草稿与列表位置，预览不被详情遮挡', async (page) => {
  await page.getByRole('article', { name: '正在认真摸鱼', exact: true }).click()
  await page.getByRole('button', { name: '详情', exact: true }).click()
  const description = page.getByLabel('我的描述')
  await description.fill('缩放窗口时保留的草稿')
  for (const width of [940, 800]) {
    await page.setViewportSize({ width, height: 600 })
    const layout = await page.evaluate(() => {
      const main = document.querySelector('main')
      return {
        mainRight: main.getBoundingClientRect().right,
        detailsLeft: document.querySelector('.details-panel').getBoundingClientRect().left,
        overflow: main.scrollWidth > main.clientWidth,
      }
    })
    assert.ok(layout.mainRight <= layout.detailsLeft)
    assert.equal(layout.overflow, false)
  }
  await page.getByRole('button', { name: '放大预览' }).click()
  const preview = page.getByRole('dialog', { name: '原图预览', exact: true })
  await preview.waitFor()
  // The dialog must receive pointer events even over the details column.
  assert.equal(
    await preview.evaluate((dialog) => {
      const bounds = dialog.getBoundingClientRect()
      return dialog.contains(document.elementFromPoint(bounds.right - 30, bounds.top + 120))
    }),
    true,
  )
  await page.getByRole('button', { name: '关闭预览' }).click()
  await preview.waitFor({ state: 'hidden' })
  await page.setViewportSize({ width: 640, height: 480 })
  const back = page.getByRole('button', { name: '返回列表' })
  await back.waitFor()
  assert.equal(await page.locator('main').isVisible(), false)
  assert.equal(await description.inputValue(), '缩放窗口时保留的草稿')
  await page.getByRole('button', { name: '放大预览' }).click()
  await preview.waitFor()
  assert.equal(
    await preview.evaluate((dialog) => {
      const bounds = dialog.getBoundingClientRect()
      return (
        bounds.width <= window.innerWidth &&
        bounds.height <= window.innerHeight &&
        dialog.scrollHeight <= dialog.clientHeight
      )
    }),
    true,
  )
  await page.keyboard.press('Escape')
  await preview.waitFor({ state: 'hidden' })
  assert.equal(await back.isVisible(), true)
  await back.click()
  await page.locator('main').waitFor()
  assert.ok(
    await page.evaluate(() =>
      window.testCalls.some(
        (call) => call.command === 'save_asset' && call.args.description === '缩放窗口时保留的草稿',
      ),
    ),
  )
  await page.getByRole('tab', { name: '关键词', exact: true }).click()
  const last = page.locator('.asset-card').last()
  await last.dblclick()
  assert.equal(await page.locator('.details-panel').count(), 0)
  await page.waitForFunction(() => window.testCalls.some((call) => call.command === 'copy_asset'))
  const scrollTop = await page.locator('main').evaluate((main) => main.scrollTop)
  assert.ok(scrollTop > 0)
  await last.click({ button: 'right' })
  await page.getByRole('menuitem', { name: '编辑描述与分组' }).click()
  await back.waitFor()
  await back.click()
  await page.locator('main').waitFor()
  assert.equal(await page.locator('main').evaluate((main) => main.scrollTop), scrollTop)
  assert.equal(
    await page.getByRole('tab', { name: '关键词', exact: true }).getAttribute('aria-selected'),
    'true',
  )
  await page.keyboard.press('Control+k')
  await page.getByRole('button', { name: '详情', exact: true }).click()
  await back.waitFor()
  await description.fill('从小窗返回宽窗的草稿')
  await page.setViewportSize({ width: 1320, height: 860 })
  await page.getByRole('button', { name: '关闭详情' }).waitFor()
  assert.equal(await description.inputValue(), '从小窗返回宽窗的草稿')
  assert.equal(await page.locator('main').isVisible(), true)
  await page.getByRole('button', { name: '关闭详情' }).click()
  await page.setViewportSize({ width: 640, height: 480 })
  await page.getByRole('article', { name: '正在认真摸鱼', exact: true }).click()
  assert.equal(await page.locator('.details-panel').count(), 0)
  await page.setViewportSize({ width: 1320, height: 860 })
  await page.getByRole('article', { name: '谢谢老板', exact: true }).click()
  assert.equal(await page.locator('.details-panel').count(), 0)
})

desktopTest('复制缓存显示占用、保留清理结果并呈现错误', async (page) => {
  await page.setViewportSize({ width: 640, height: 480 })
  await page.getByRole('button', { name: '偏好设置', exact: true }).click()
  const cache = page.getByRole('region', { name: '复制缓存', exact: true })
  await cache.getByText('32.0 MB · 24 个文件 · 软上限 512 MiB', { exact: true }).waitFor()
  const clear = cache.getByRole('button', { name: '清理复制缓存', exact: true })
  await clear.click()
  await cache.getByText('2.0 MB · 1 个文件 · 软上限 512 MiB', { exact: true }).waitFor()
  assert.ok(
    await page.evaluate(() => window.testCalls.some((call) => call.command === 'clear_copy_cache')),
  )
  await page.evaluate(() => {
    window.testCopyCache.error = '无法读取当前剪贴板文件，已跳过缓存清理'
  })
  await clear.click()
  await cache.getByRole('alert').filter({ hasText: '已跳过缓存清理' }).waitFor()
  await page.evaluate(() => {
    window.testCopyCache.error = null
  })
  await cache.getByRole('button', { name: '刷新', exact: true }).click()
  await cache.getByRole('alert').waitFor({ state: 'hidden' })
  await cache.getByText('2.0 MB · 1 个文件 · 软上限 512 MiB', { exact: true }).waitFor()
})

desktopTest('设置草稿、本地模型与图片理解配置', async (page) => {
  await page.evaluate(() => {
    window.testModel.ready = false
    window.testModel.missing = ['qwen3-embedding-0.6b/memechan-model.json']
    window.testModelImportResult = false
  })
  await page.getByRole('button', { name: '偏好设置' }).click()
  await page.getByLabel('默认复制最长边').fill('640')
  const snapshots = await page.evaluate(
    () => window.testCalls.filter((call) => call.command === 'snapshot').length,
  )
  await page.evaluate(() => {
    window.testModel.job.done++
    window.testModel.libraryRevision++
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
  const embeddingModel = page.getByRole('combobox', { name: '向量模型', exact: true })
  for (const [label, value] of [
    ['Qwen3-Embedding-4B', 'qwen3-4b'],
    ['Qwen3-Embedding-8B', 'qwen3-8b'],
  ]) {
    await embeddingModel.click()
    await page.getByRole('option', { name: label, exact: true }).click()
    await page.waitForFunction(
      (expected) =>
        window.testCalls.some(
          (call) =>
            call.command === 'save_settings' && call.args.settings.embeddingModel === expected,
        ),
      value,
    )
    await page.getByText('待安装', { exact: true }).waitFor()
  }
  await page.getByRole('button', { name: '关闭弹窗', exact: true }).click()
  await page.getByRole('button', { name: '偏好设置', exact: true }).click()
  await page.getByRole('tab', { name: 'Embedding', exact: true }).click()
  assert.match(await embeddingModel.innerText(), /Qwen3-Embedding-8B/)
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
  await page.getByRole('button', { name: '详情', exact: true }).click()
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
  await page.getByRole('article', { name: '今天也要开心', exact: true }).click()
  await page.getByRole('button', { name: '详情', exact: true }).click()
  await page.getByText('文字已清除，可重新识别。', { exact: true }).waitFor()
  await page.getByRole('article', { name: '正在认真摸鱼', exact: true }).click()
  await page.getByText('文字已清除，可重新识别。', { exact: true }).waitFor()
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
  await page.getByRole('button', { name: '详情', exact: true }).click()
  await page.getByLabel('我的描述').waitFor()
  await page.evaluate(() => window.testResumeImport())
  await page.getByText('已添加 3 张表情', { exact: true }).waitFor()
  assert.equal(await page.locator('.asset-card').count(), 9)
})
