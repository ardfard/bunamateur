import { expect, test } from '@playwright/test'

test.describe('BunAmateur Core Flows', () => {
  test('serves PWA manifest and Hanko icon', async ({ request }) => {
    const manifestRes = await request.get('/manifest.webmanifest')
    expect(manifestRes.ok()).toBeTruthy()
    const manifest = await manifestRes.json()
    expect(manifest.name).toBe('BunAmateur (文アマ)')
    expect(manifest.short_name).toBe('BunAmateur')

    const iconRes = await request.get('/icon.svg')
    expect(iconRes.ok()).toBeTruthy()
    expect(await iconRes.text()).toContain('文')
  })

  test('renders auth page, registers user, navigates routes, and tests IME + grammar deep link', async ({
    page,
  }) => {
    const suffix = Date.now()
    const email = `e2e-${suffix}@example.com`
    const username = `e2euser${suffix}`
    const password = 'TestPassword123!'

    await page.goto('/')
    await expect(page.getByText('BunAmateur')).toBeVisible()

    // Switch to register mode if currently on sign in
    const createAccountBtn = page.getByRole('button', { name: /create account|register|sign up/i })
    if (await createAccountBtn.isVisible()) {
      await createAccountBtn.click()
    }

    await page.getByPlaceholder(/email/i).fill(email)
    const usernameField = page.getByPlaceholder(/username/i)
    if (await usernameField.isVisible()) {
      await usernameField.fill(username)
    }
    await page.getByPlaceholder(/password/i).fill(password)
    await page.getByRole('button', { name: /create account|register|sign in/i }).first().click()

    // Wait for authenticated top navigation
    const nav = page.getByRole('navigation', { name: 'Primary' })
    await expect(nav).toBeVisible({ timeout: 10_000 })

    // Navigate via History API links
    await nav.getByRole('link', { name: 'Grammar' }).click()
    await expect(page).toHaveURL(/\/grammar$/)
    await expect(page.getByRole('heading', { name: 'Grammar Library' })).toBeVisible()

    // Open first grammar point and verify URL updates to /grammar/:slug
    const firstCard = page.locator('main button').filter({ hasText: /JLPT|N5|N4|N3|N2|N1/ }).first()
    await firstCard.click()
    await expect(page).toHaveURL(/\/grammar\/[^/]+$/)
    await expect(page.getByText('Structure / Formation')).toBeVisible()

    // Reload deep link directly and verify modal re-opens
    const deepUrl = page.url()
    await page.reload()
    await expect(page).toHaveURL(deepUrl)
    await expect(page.getByText('Structure / Formation')).toBeVisible({ timeout: 10_000 })

    // Navigate to Conjugation Dojo and verify IME mode cycling + live conversion
    await nav.getByRole('link', { name: 'Conjugation' }).click()
    await expect(page).toHaveURL(/\/conjugation$/)
    const conjInput = page.locator('form input[type="text"]')
    await conjInput.fill('tabete')
    await expect(conjInput).toHaveValue('たべて')

    // Cycle Hiragana -> Katakana -> Off
    const imeBtn = page.getByRole('button', { name: /Hiragana/i })
    await imeBtn.click()
    await expect(page.getByRole('button', { name: /Katakana/i })).toBeVisible()
    await conjInput.fill('kappu')
    await expect(conjInput).toHaveValue('カップ')

    // Navigate to Reviews and verify shortcut legend
    await nav.getByRole('link', { name: /Reviews/i }).click()
    await expect(page).toHaveURL(/\/reviews/)
    await page.getByRole('button', { name: 'Cram Mode' }).click()
    await expect(page).toHaveURL(/\/reviews\?mode=cram/)
    await expect(page.getByLabel('Keyboard shortcuts')).toBeVisible({ timeout: 10_000 })
  })
})
