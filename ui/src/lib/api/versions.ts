import { apiFetch, encodeObjectKey } from './http'

export interface Version {
  versionId: string | null
  lastModified: string
  size: number
  etag: string
  isDeleteMarker: boolean
}

export interface VersionsResponse { versions: Version[] }

export async function listVersions(bucket: string, objectKey: string): Promise<VersionsResponse> {
  return apiFetch<VersionsResponse>(`/api/buckets/${encodeURIComponent(bucket)}/versions?key=${encodeURIComponent(objectKey)}`)
}

export async function deleteVersion(bucket: string, objectKey: string, versionId: string): Promise<{ ok: boolean }> {
  return apiFetch<{ ok: boolean }>(`/api/buckets/${encodeURIComponent(bucket)}/versions/${encodeURIComponent(versionId)}/objects/${encodeObjectKey(objectKey)}`, { method: 'DELETE' })
}
