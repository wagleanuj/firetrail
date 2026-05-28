import { test, expect } from '@playwright/test'

/**
 * Happy path for the memory UI:
 *   1. /memory shows the empty state on a fresh workspace.
 *   2. Open create dialog, switch to the Gotcha kind, fill the summary,
 *      submit.
 *   3. Card appears in the list.
 *   4. Click the card → /memory/:id renders with the same title.
 *   5. Search for a substring → result lands on the search page; click
 *      through and end up back on the detail page.
 *
 * As with board.spec.ts, the test exercises the bundled-ui binary.
 * Run with `pnpm test:e2e`.
 */
test.describe('memory happy path', () => {
  test('create → list → detail → search', async ({ page }) => {
    await page.goto('/memory')

    // 1. Empty state.
    await expect(page.getByText(/no memory yet/i)).toBeVisible()

    // 2. Open create modal → Gotcha tab → fill + submit.
    await page.getByRole('button', { name: /create memory/i }).click()
    await page.getByRole('tab', { name: /gotcha/i }).click()
    await page
      .getByPlaceholder('The trap, in one line')
      .fill('Mind the leaky abstraction')
    await page.getByRole('button', { name: /^create$/i }).click()

    // 3. Card appears in the list.
    await expect(
      page.getByTestId('memory-list').getByText('Mind the leaky abstraction'),
    ).toBeVisible()

    // 4. Click → detail page.
    await page.getByText('Mind the leaky abstraction').click()
    await expect(page).toHaveURL(/\/memory\/[a-z0-9:]+/i)
    await expect(
      page.getByRole('heading', { name: 'Mind the leaky abstraction' }),
    ).toBeVisible()

    // 5. Search for the substring.
    await page.goto('/memory/search?q=leaky&mode=lexical')
    await expect(page.getByTestId('search-results')).toBeVisible()
    await page
      .getByTestId('search-results')
      .getByText('Mind the leaky abstraction')
      .click()
    await expect(page).toHaveURL(/\/memory\/[a-z0-9:]+/i)
  })
})
