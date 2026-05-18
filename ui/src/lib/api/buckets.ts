import { apiFetch } from './http'

export interface Bucket {
  name: string
  createdAt: string
  versioning: boolean
  encryption: boolean
}

export interface BucketsResponse { buckets: Bucket[] }

export async function listBuckets(): Promise<BucketsResponse> {
  return apiFetch<BucketsResponse>('/api/buckets')
}

export async function createBucket(name: string): Promise<{ ok: boolean }> {
  return apiFetch<{ ok: boolean }>('/api/buckets', {
    method: 'POST',
    body: JSON.stringify({ name }),
  })
}

export async function deleteBucket(name: string): Promise<{ ok: boolean }> {
  return apiFetch<{ ok: boolean }>(`/api/buckets/${encodeURIComponent(name)}`, { method: 'DELETE' })
}
