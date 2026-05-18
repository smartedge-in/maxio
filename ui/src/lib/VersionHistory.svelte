<script lang="ts">
  import { createMutation, createQuery } from '@tanstack/svelte-query'
  import * as Table from '$lib/components/ui/table'
  import { Button } from '$lib/components/ui/button'
  import { Callout } from '$lib/components/ui/callout'
  import { ConfirmDialog } from '$lib/components/ui/confirm-dialog'
  import Download from 'lucide-svelte/icons/download'
  import Trash2 from 'lucide-svelte/icons/trash-2'
  import Tag from 'lucide-svelte/icons/tag'
  import Loader2 from 'lucide-svelte/icons/loader-2'
  import { versionKeys } from '$lib/api/keys'
  import { deleteVersion as deleteVersionApi, listVersions } from '$lib/api/versions'
  import { ApiError, encodeObjectKey } from '$lib/api/http'
  import { queryClient } from '$lib/query/client'

  interface Props {
    bucket: string
    objectKey: string
    onClose: () => void
    onVersionDeleted?: () => void
  }
  let { bucket, objectKey, onClose, onVersionDeleted }: Props = $props()

  const versionsQuery = createQuery(() => ({
    queryKey: versionKeys.list(bucket, objectKey),
    queryFn: () => listVersions(bucket, objectKey),
  }))
  const deleteVersionMutation = createMutation(() => ({
    mutationFn: (versionId: string) => deleteVersionApi(bucket, objectKey, versionId),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: versionKeys.list(bucket, objectKey) })
      onVersionDeleted?.()
    },
  }))

  const versions = $derived(versionsQuery.data?.versions ?? [])
  let deleteError = $state<string | null>(null)
  let versionToDelete = $state<string | null>(null)

  async function deleteVersion(versionId: string) {
    versionToDelete = versionId
  }

  async function confirmDeleteVersion() {
    if (!versionToDelete) return
    try {
      deleteError = null
      await deleteVersionMutation.mutateAsync(versionToDelete)
      versionToDelete = null
    } catch (err) {
      console.error('deleteVersion failed:', err)
      deleteError = err instanceof ApiError ? err.message : 'Failed to connect to server'
    }
  }

  function downloadVersion(versionId: string) {
    window.open(
      `/api/buckets/${encodeURIComponent(bucket)}/versions/${encodeURIComponent(versionId)}/download/${encodeObjectKey(objectKey)}`,
      '_blank'
    )
  }

  function formatSize(bytes: number): string {
    if (bytes === 0) return '0 B'
    const k = 1024
    const sizes = ['B', 'KB', 'MB', 'GB']
    const i = Math.floor(Math.log(bytes) / Math.log(k))
    return parseFloat((bytes / Math.pow(k, i)).toFixed(1)) + ' ' + sizes[i]
  }

  function formatDate(iso: string): string {
    try {
      return new Date(iso).toLocaleString()
    } catch {
      return iso
    }
  }

  function truncateId(id: string): string {
    return id.length > 16 ? id.slice(0, 16) + '...' : id
  }
</script>

<div class="rounded-sm border bg-card">
  <div class="flex items-center justify-between border-b px-4 py-2">
    <h4 class="text-sm font-semibold">Version History</h4>
    <Button variant="ghost" size="sm" onclick={onClose}>Close</Button>
  </div>

  {#if versionsQuery.isError || deleteError}
    <div class="p-4"><Callout type="danger">{deleteError ?? (versionsQuery.error instanceof ApiError ? versionsQuery.error.message : 'Failed to load versions')}</Callout></div>
  {/if}

  {#if versionsQuery.isPending}
    <div class="flex items-center gap-2 px-4 py-4 text-sm text-muted-foreground">
      <Loader2 class="size-4 animate-spin" /> Loading versions...
    </div>
  {:else if versions.length === 0}
    <div class="px-4 py-4 text-sm text-muted-foreground">No versions found.</div>
  {:else}
    <Table.Root>
      <Table.Header>
        <Table.Row>
          <Table.Head>Version ID</Table.Head>
          <Table.Head>Date</Table.Head>
          <Table.Head>Size</Table.Head>
          <Table.Head>Type</Table.Head>
          <Table.Head class="w-20"></Table.Head>
        </Table.Row>
      </Table.Header>
      <Table.Body>
        {#each versions as version, i}
          <Table.Row class={version.isDeleteMarker ? 'opacity-60' : ''}>
            <Table.Cell class="font-mono text-xs">
              <span title={version.versionId ?? ''}>
                {version.versionId ? truncateId(version.versionId) : 'null'}
              </span>
              {#if i === 0}
                <span class="ml-1 rounded-sm bg-accent/20 px-1 py-0.5 text-[10px] font-medium text-accent-foreground">latest</span>
              {/if}
            </Table.Cell>
            <Table.Cell class="text-muted-foreground text-xs">{formatDate(version.lastModified)}</Table.Cell>
            <Table.Cell class="text-muted-foreground text-xs">
              {version.isDeleteMarker ? '—' : formatSize(version.size)}
            </Table.Cell>
            <Table.Cell>
              {#if version.isDeleteMarker}
                <span class="inline-flex items-center gap-1 rounded-sm bg-destructive/10 px-1.5 py-0.5 text-[10px] font-medium text-destructive">
                  <Tag class="size-3" /> Delete Marker
                </span>
              {:else}
                <span class="text-xs text-muted-foreground">Version</span>
              {/if}
            </Table.Cell>
            <Table.Cell class="w-20">
              <div class="flex items-center gap-4">
                {#if !version.isDeleteMarker && version.versionId}
                  <button
                    class="text-muted-foreground hover:text-foreground transition-colors"
                    onclick={() => downloadVersion(version.versionId!)}
                    title="Download this version"
                  >
                    <Download class="size-4" />
                  </button>
                {/if}
                {#if version.versionId}
                  <button
                    class="text-muted-foreground hover:text-destructive transition-colors"
                    onclick={() => deleteVersion(version.versionId!)}
                    title="Permanently delete this version"
                  >
                    <Trash2 class="size-4" />
                  </button>
                {/if}
              </div>
            </Table.Cell>
          </Table.Row>
        {/each}
      </Table.Body>
    </Table.Root>
  {/if}
</div>

<ConfirmDialog
  open={versionToDelete !== null}
  title="Permanently delete version?"
  description="This object version will be permanently deleted. This cannot be undone."
  confirmLabel="Delete version"
  confirmVariant="destructive"
  confirmationText="delete"
  confirmationLabel="Type delete"
  loading={deleteVersionMutation.isPending}
  onClose={() => (versionToDelete = null)}
  onConfirm={confirmDeleteVersion}
/>
