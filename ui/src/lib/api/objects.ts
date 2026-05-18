import { apiFetch, encodeObjectKey } from './http'

export interface S3File {
  key: string
  size: number
  lastModified: string
  etag: string
}

export interface ObjectsResponse {
  files: S3File[]
  prefixes: string[]
  emptyPrefixes?: string[]
}

export async function listObjects(bucket: string, prefix: string): Promise<ObjectsResponse> {
  const params = new URLSearchParams({ prefix, delimiter: '/' })
  return apiFetch<ObjectsResponse>(`/api/buckets/${encodeURIComponent(bucket)}/objects?${params}`)
}

export async function uploadObject(bucket: string, key: string, file: File): Promise<{ ok: boolean }> {
  const res = await fetch(`/api/buckets/${encodeURIComponent(bucket)}/upload/${encodeObjectKey(key)}`, {
    method: 'PUT',
    body: file,
    credentials: 'same-origin',
  })
  if (!res.ok) throw new Error(`Upload failed (${res.status})`)
  return { ok: true }
}

export async function deleteObject(bucket: string, key: string): Promise<{ ok: boolean }> {
  return apiFetch<{ ok: boolean }>(`/api/buckets/${encodeURIComponent(bucket)}/objects/${encodeObjectKey(key)}`, { method: 'DELETE' })
}

export async function createFolder(bucket: string, name: string): Promise<{ ok: boolean }> {
  return apiFetch<{ ok: boolean }>(`/api/buckets/${encodeURIComponent(bucket)}/folders`, {
    method: 'POST',
    body: JSON.stringify({ name }),
  })
}

export async function presignObject(bucket: string, key: string, expires: number): Promise<{ url: string }> {
  return apiFetch<{ url: string }>(`/api/buckets/${encodeURIComponent(bucket)}/presign/${encodeObjectKey(key)}?expires=${expires}`)
}
