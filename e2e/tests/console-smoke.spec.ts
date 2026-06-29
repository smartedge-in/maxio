import { expect, test } from '@playwright/test';

const accessKey = process.env.MAXIO_ACCESS_KEY ?? 'maxioadmin';
const secretKey = process.env.MAXIO_SECRET_KEY ?? 'maxioadmin';

test('console login → bucket → upload → download → delete', async ({ page }) => {
  await page.goto('/ui/');
  await expect(page.locator('body')).toBeVisible();

  // Browser UI login (sets session cookies on the page context).
  await page.locator('#accessKey').fill(accessKey);
  await page.locator('#secretKey').fill(secretKey);
  await page.getByRole('button', { name: 'Login' }).click();
  await expect(page.getByRole('heading', { name: 'Buckets' })).toBeVisible({ timeout: 15_000 });

  const api = page.request;
  const bucket = `e2e-${Date.now()}`;

  const create = await api.post('/api/buckets', {
    data: { name: bucket },
  });
  expect(create.ok()).toBeTruthy();

  const payload = `playwright-smoke-${Date.now()}`;
  const upload = await api.put(`/api/buckets/${bucket}/upload/smoke.txt`, {
    data: payload,
  });
  expect(upload.ok()).toBeTruthy();

  const download = await api.get(`/api/buckets/${bucket}/download/smoke.txt`);
  expect(download.ok()).toBeTruthy();
  expect(await download.text()).toBe(payload);

  const delObj = await api.delete(`/api/buckets/${bucket}/objects/smoke.txt`);
  expect(delObj.ok()).toBeTruthy();

  const rmBucket = await api.delete(`/api/buckets/${bucket}`);
  expect(rmBucket.ok()).toBeTruthy();
});