import { apiFetch } from './http'

export interface EnabledResponse { enabled: boolean }
export interface PublicAccessResponse { read: boolean; list: boolean }

export async function getVersioning(bucket: string): Promise<EnabledResponse> {
  return apiFetch<EnabledResponse>(`/api/buckets/${encodeURIComponent(bucket)}/versioning`)
}

export async function setVersioning(bucket: string, enabled: boolean): Promise<EnabledResponse> {
  return apiFetch<EnabledResponse>(`/api/buckets/${encodeURIComponent(bucket)}/versioning`, {
    method: 'PUT',
    body: JSON.stringify({ enabled }),
  })
}

export async function getEncryption(bucket: string): Promise<EnabledResponse> {
  return apiFetch<EnabledResponse>(`/api/buckets/${encodeURIComponent(bucket)}/encryption`)
}

export async function setEncryption(bucket: string, enabled: boolean): Promise<EnabledResponse> {
  return apiFetch<EnabledResponse>(`/api/buckets/${encodeURIComponent(bucket)}/encryption`, {
    method: 'PUT',
    body: JSON.stringify({ enabled }),
  })
}

export async function getPublicAccess(bucket: string): Promise<PublicAccessResponse> {
  return apiFetch<PublicAccessResponse>(`/api/buckets/${encodeURIComponent(bucket)}/public`)
}

export async function setPublicAccess(bucket: string, read: boolean, list: boolean): Promise<PublicAccessResponse> {
  return apiFetch<PublicAccessResponse>(`/api/buckets/${encodeURIComponent(bucket)}/public`, {
    method: 'PUT',
    body: JSON.stringify({ read, list }),
  })
}
