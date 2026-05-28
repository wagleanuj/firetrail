import { test, expect } from '@playwright/test'

/**
 * Happy path for the kanban UI:
 *   1. Empty state on a fresh workspace.
 *   2. Open create dialog, fill title, submit.
 *   3. New card appears in Todo.
 *   4. Drag card to In Progress; status persists across reload.
 *   5. Open the card → drawer → click Claim → status updates.
 *
 * The test exercises the bundled-ui binary (see playwright.config.ts'
 * `webServer`). Run with `pnpm test:e2e`.
 */
test.describe('kanban happy path', () => {
  test('create → drag → reload → claim', async ({ page }) => {
    await page.goto('/')

    // 1. Empty state.
    await expect(page.getByText(/no tickets yet/i)).toBeVisible()

    // 2. Open create modal.
    await page.getByRole('button', { name: /create ticket/i }).click()
    await page.getByPlaceholder('Short, imperative').fill('Wire e2e check')
    await page.getByRole('button', { name: /^create$/i }).click()

    // 3. Card lands in Todo.
    const todo = page.getByTestId('column-todo')
    await expect(todo.getByText('Wire e2e check')).toBeVisible()

    // 4. Drag the new card from Todo to In Progress.
    const card = todo.getByText('Wire e2e check')
    await card.dragTo(page.getByTestId('column-in_progress'))
    const inProgress = page.getByTestId('column-in_progress')
    await expect(inProgress.getByText('Wire e2e check')).toBeVisible()

    await page.reload()
    await expect(page.getByTestId('column-in_progress').getByText('Wire e2e check')).toBeVisible()

    // 5. Open drawer + claim.
    await page.getByText('Wire e2e check').click()
    await page.getByRole('button', { name: /^claim$/i }).click()
    await expect(page.getByRole('button', { name: /^unclaim$/i })).toBeVisible()
  })
})
