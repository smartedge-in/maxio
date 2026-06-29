import { expect, test } from '@playwright/test';

const accessKey = process.env.MAXIO_ACCESS_KEY ?? 'maxioadmin';
const secretKey = process.env.MAXIO_SECRET_KEY ?? 'maxioadmin';

test('console login → bucket → upload → download → delete', async ({ page, request }) => {
  await page.goto('/ui/');
  await expect(page.locator('body')).toBeVisible();

  const loginResp = await request.post('/api/auth/login', {
    data: { accessKey, secretKey },
  });
  expect(loginResp.ok()).toBeTruthy();
  const { token } = (await loginResp.json()) as { token: string };
  expect(token.length).toBeGreaterThan(0);

  const bucket = `e2e-${Date.now()}`;
  const auth = { Authorization: `Bearer ${token}` };

  const create = await request.post('/api/buckets', {
    headers: auth,
    data: { name: bucket },
  });
  expect(create.ok()).toBeTruthy();

  const payload = `playwright-smoke-${Date.now()}`;
  const upload = await request.put(`/api/buckets/${bucket}/upload/smoke.txt`, {
    headers: auth,
    data: payload,
  });
  expect(upload.ok()).toBeTruthy();

  const download = await request.get(`/api/buckets/${bucket}/download/smoke.txt`, {
    headers: auth,
  });
  expect(download.ok()).toBeTruthy();
  expect(await download.text()).toBe(payload);

  const delObj = await request.delete(`/api/buckets/${bucket}/objects/smoke.txt`, {
    headers: auth,
  });
  expect(delObj.ok()).toBeTruthy();

  const rmBucket = await request.delete(`/api/buckets/${bucket}`, { headers: auth });
  expect(rmBucket.ok()).toBeTruthy();
});