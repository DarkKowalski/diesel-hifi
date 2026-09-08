import type { Page } from '@playwright/test';

export async function chooseOption(page: Page, id: 'language-select' | 'engine-select', value: string) {
  await page.getByTestId(id).click();
  await page.locator(`#${id}-options [role="option"][data-value="${value}"]`).click();
}
