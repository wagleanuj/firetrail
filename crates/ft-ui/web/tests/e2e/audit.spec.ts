import { test, expect } from '@playwright/test'

/**
 * Happy path for the audit UI: dashboard → lint → run → see findings.
 *
 * A fresh workspace usually has zero findings, so we don't assert on the
 * row count — just on the dashboard tile + the lint summary line rendering
 * after a successful run.
 */
test.describe('audit happy path', () => {
  test('dashboard → lint → run', async ({ page }) => {
    await page.goto('/audit')
    await expect(page.getByTestId('audit-tile-lint')).toBeVisible()
    await page.getByTestId('audit-tile-lint').click()
    await expect(page).toHaveURL(/\/audit\/lint/)

    await page.getByTestId('lint-run').click()
    await expect(page.getByTestId('lint-summary')).toBeVisible()
  })

  test('graph tile is reachable', async ({ page }) => {
    await page.goto('/audit')
    await page.getByTestId('audit-tile-graph').click()
    await expect(page).toHaveURL(/\/audit\/graph/)
  })
})
