<script lang="ts">
  import { onMount } from 'svelte'
  import { createMutation, createQuery } from '@tanstack/svelte-query'
  import * as Table from '$lib/components/ui/table'
  import { Button } from '$lib/components/ui/button'
  import { Callout } from '$lib/components/ui/callout'
  import { ConfirmDialog } from '$lib/components/ui/confirm-dialog'
  import { Dialog } from '$lib/components/ui/dialog'
  import { Input } from '$lib/components/ui/input'
  import Folder from 'lucide-svelte/icons/folder'
  import FileIcon from 'lucide-svelte/icons/file'
  import Download from 'lucide-svelte/icons/download'
  import Upload from 'lucide-svelte/icons/upload'
  import Trash2 from 'lucide-svelte/icons/trash-2'
  import Share2 from 'lucide-svelte/icons/share-2'
  import Check from 'lucide-svelte/icons/check'
  import FolderPlus from 'lucide-svelte/icons/folder-plus'
  import History from 'lucide-svelte/icons/history'
  import VersionHistory from './VersionHistory.svelte'
  import { toast } from '$lib/toast'
  import { objectKeys, settingsKeys } from '$lib/api/keys'
  import { createFolder as createFolderApi, deleteObject as deleteObjectApi, listObjects, presignObject, uploadObject } from '$lib/api/objects'
  import { getVersioning } from '$lib/api/settings'
  import { ApiError, encodeObjectKey } from '$lib/api/http'
  import { queryClient } from '$lib/query/client'

  interface Props {
    bucket: string
    onBack: () => void
    onPrefixChange?: (prefix: string, breadcrumbs: { label: string; prefix: string }[]) => void
  }
  let { bucket, onBack, onPrefixChange }: Props = $props()

  let prefix = $state('')
  let fileInput: HTMLInputElement | undefined = $state()
  let copiedKey = $state<string | null>(null)
  let shareMenuKey = $state<string | null>(null)
  let showCreateFolder = $state(false)
  let newFolderName = $state('')
  let shareMenuPos = $state({ top: 0, left: 0 })
  let versionKey = $state<string | null>(null)
  let pendingDelete = $state<{ key: string; kind: 'object' | 'folder' } | null>(null)
  let createFolderInput = $state<HTMLInputElement | null>(null)

  $effect(() => {
    if (showCreateFolder && createFolderInput) {
      queueMicrotask(() => createFolderInput?.focus())
    }
  })

  const objectsQuery = createQuery(() => ({
    queryKey: objectKeys.list(bucket, prefix),
    queryFn: () => listObjects(bucket, prefix),
  }))

  const versioningQuery = createQuery(() => ({
    queryKey: settingsKeys.versioning(bucket),
    queryFn: () => getVersioning(bucket),
  }))

  const uploadMutation = createMutation(() => ({
    mutationFn: async (files: FileList) => {
      for (const file of Array.from(files)) {
        await uploadObject(bucket, `${prefix}${file.name}`, file)
      }
      return files.length
    },
    onSuccess: (count) => {
      toast.success(count === 1 ? 'File uploaded' : `${count} files uploaded`)
      if (fileInput) fileInput.value = ''
      queryClient.invalidateQueries({ queryKey: objectKeys.list(bucket, prefix) })
    },
  }))

  const deleteObjectMutation = createMutation(() => ({
    mutationFn: (key: string) => deleteObjectApi(bucket, key),
    onSuccess: (_data, key) => {
      toast.success(`"${displayName(key)}" deleted`)
      queryClient.invalidateQueries({ queryKey: objectKeys.list(bucket, prefix) })
    },
  }))

  const createFolderMutation = createMutation(() => ({
    mutationFn: (name: string) => createFolderApi(bucket, `${prefix}${name}`),
    onSuccess: (_data, name) => {
      toast.success(`Folder "${name}" created`)
      newFolderName = ''
      showCreateFolder = false
      queryClient.invalidateQueries({ queryKey: objectKeys.list(bucket, prefix) })
    },
  }))

  const files = $derived(objectsQuery.data?.files ?? [])
  const prefixes = $derived(objectsQuery.data?.prefixes ?? [])
  const emptyPrefixes = $derived(new Set(objectsQuery.data?.emptyPrefixes ?? []))
  const versioningEnabled = $derived(!!versioningQuery.data?.enabled)

  const expiryOptions = [
    { label: '1 hour', seconds: 3600 },
    { label: '6 hours', seconds: 21600 },
    { label: '24 hours', seconds: 86400 },
    { label: '7 days', seconds: 604800 },
  ]


  function notifyPrefix() {
    onPrefixChange?.(prefix, breadcrumbs)
  }

  export function navigateTo(newPrefix: string) {
    prefix = newPrefix
    notifyPrefix()
  }

  export function goUp() {
    if (!prefix) {
      onBack()
      return
    }
    const trimmed = prefix.slice(0, -1)
    const lastSlash = trimmed.lastIndexOf('/')
    prefix = lastSlash >= 0 ? trimmed.slice(0, lastSlash + 1) : ''
    notifyPrefix()
  }

  function displayName(fullPath: string): string {
    const trimmed = fullPath.endsWith('/') ? fullPath.slice(0, -1) : fullPath
    const lastSlash = trimmed.lastIndexOf('/')
    return lastSlash >= 0 ? trimmed.slice(lastSlash + 1) : trimmed
  }

  function formatSize(bytes: number): string {
    if (bytes < 1024) return `${bytes} B`
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`
    if (bytes < 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MB`
    return `${(bytes / (1024 * 1024 * 1024)).toFixed(1)} GB`
  }

  function formatDate(iso: string): string {
    try {
      return new Date(iso).toLocaleString()
    } catch {
      return iso
    }
  }

  let breadcrumbs = $derived.by(() => {
    const parts = prefix.split('/').filter(Boolean)
    const crumbs: { label: string; prefix: string }[] = [
      { label: bucket, prefix: '' },
    ]
    let acc = ''
    for (const part of parts) {
      acc += part + '/'
      crumbs.push({ label: part, prefix: acc })
    }
    return crumbs
  })

  function downloadUrl(key: string): string {
    return `/api/buckets/${encodeURIComponent(bucket)}/download/${encodeObjectKey(key)}`
  }

  async function handleUpload() {
    const inputFiles = fileInput?.files
    if (!inputFiles || inputFiles.length === 0) return
    const toastId = toast.loading(inputFiles.length === 1 ? `Uploading ${inputFiles[0].name}…` : `Uploading ${inputFiles.length} files…`)
    try {
      await uploadMutation.mutateAsync(inputFiles)
      toast.dismiss(toastId)
    } catch (err) {
      console.error('Upload failed:', err)
      toast.error(err instanceof Error ? err.message : 'Upload failed', { id: toastId })
      if (fileInput) fileInput.value = ''
    }
  }

  async function deleteObject(key: string, e: Event) {
    e.stopPropagation()
    pendingDelete = { key, kind: 'object' }
  }

  async function confirmPendingDelete() {
    if (!pendingDelete) return
    const { key, kind } = pendingDelete
    try {
      await deleteObjectMutation.mutateAsync(key)
      pendingDelete = null
    } catch (err) {
      console.error(kind === 'folder' ? 'deleteFolder failed:' : 'deleteObject failed:', err)
      toast.error(err instanceof ApiError ? err.message : kind === 'folder' ? 'Failed to delete folder' : 'Failed to connect to server')
    }
  }

  function toggleShareMenu(key: string, e: MouseEvent) {
    e.stopPropagation()
    if (shareMenuKey === key) {
      shareMenuKey = null
      return
    }
    const btn = e.currentTarget as HTMLElement
    const rect = btn.getBoundingClientRect()
    shareMenuPos = { top: rect.top, left: rect.right }
    shareMenuKey = key
  }

  async function shareObject(key: string, expires: number) {
    shareMenuKey = null
    try {
      const data = await presignObject(bucket, key, expires)
      await navigator.clipboard.writeText(data.url)
      copiedKey = key
      setTimeout(() => { copiedKey = null }, 2000)
      toast.success('Presigned URL copied to clipboard')
    } catch (err) {
      console.error('shareObject failed:', err)
      toast.error(err instanceof ApiError ? err.message : 'Failed to generate share link')
    }
  }

  async function createFolder() {
    const name = newFolderName.trim()
    if (!name) return
    try {
      await createFolderMutation.mutateAsync(name)
    } catch (err) {
      console.error('createFolder failed:', err)
      toast.error(err instanceof ApiError ? err.message : 'Failed to create folder')
    }
  }

  async function deleteFolder(folderPrefix: string, e: Event) {
    e.stopPropagation()
    pendingDelete = { key: folderPrefix, kind: 'folder' }
  }

  function handleClickOutside() {
    if (shareMenuKey) shareMenuKey = null
  }

  onMount(() => {
    document.addEventListener('click', handleClickOutside)
    return () => document.removeEventListener('click', handleClickOutside)
  })
</script>

<div class="flex flex-col gap-4">
  {#if objectsQuery.isError}
    <Callout type="danger">{objectsQuery.error instanceof ApiError ? objectsQuery.error.message : 'Failed to load objects'}</Callout>
  {/if}

  <div class="flex items-center gap-2">
    <input
      bind:this={fileInput}
      type="file"
      multiple
      class="hidden"
      onchange={handleUpload}
    />
    <Button variant="highlighted" class="h-8" onclick={() => fileInput?.click()} disabled={uploadMutation.isPending}>
      <Upload class="size-4 mr-1" /> {uploadMutation.isPending ? 'Uploading...' : 'Upload'}
    </Button>
    <Button variant="outline" class="h-8" onclick={() => (showCreateFolder = true)}>
      <FolderPlus class="size-4 mr-1" /> New Folder
    </Button>
  </div>

  {#if objectsQuery.isPending}
    <p class="text-sm text-muted-foreground">Loading...</p>
  {:else if files.length === 0 && prefixes.length === 0 && !objectsQuery.isError}
    <Callout type="info">
      <span class="inline-flex items-center gap-2">
        <Folder class="size-4 opacity-70" />
        This location is empty — upload a file or create a folder to get started.
      </span>
    </Callout>
  {:else}
    <Table.Root>
      <Table.Header>
        <Table.Row>
          <Table.Head>Name</Table.Head>
          <Table.Head class="w-28 text-right">Size</Table.Head>
          <Table.Head class="w-48">Modified</Table.Head>
          <Table.Head class="w-24"></Table.Head>
        </Table.Row>
      </Table.Header>
      <Table.Body>
        {#each prefixes as p}
          <Table.Row class="cursor-pointer" onclick={() => navigateTo(p)}>
            <Table.Cell>
              <span class="flex items-center gap-2">
                <Folder class="size-4 shrink-0 text-muted-foreground" />
                <span class="font-medium">{displayName(p)}/</span>
              </span>
            </Table.Cell>
            <Table.Cell class="text-right text-muted-foreground">&mdash;</Table.Cell>
            <Table.Cell class="text-muted-foreground">&mdash;</Table.Cell>
            <Table.Cell>
              {#if emptyPrefixes.has(p)}
                <button
                  class="text-muted-foreground hover:text-destructive transition-colors"
                  onclick={(e) => deleteFolder(p, e)}
                  title="Delete empty folder"
                >
                  <Trash2 class="size-4" />
                </button>
              {/if}
            </Table.Cell>
          </Table.Row>
        {/each}
        {#each files as file}
          <Table.Row>
            <Table.Cell>
              <span class="flex items-center gap-2">
                <FileIcon class="size-4 shrink-0 text-muted-foreground" />
                <span class="font-medium">{displayName(file.key)}</span>
              </span>
            </Table.Cell>
            <Table.Cell class="text-right text-muted-foreground">{formatSize(file.size)}</Table.Cell>
            <Table.Cell class="text-muted-foreground">{formatDate(file.lastModified)}</Table.Cell>
            <Table.Cell class="w-24">
              <span class="flex items-center gap-4">
                {#if versioningEnabled}
                  <button
                    class="text-muted-foreground hover:text-foreground transition-colors"
                    onclick={(e) => { e.stopPropagation(); versionKey = versionKey === file.key ? null : file.key }}
                    title="Version history"
                  >
                    <History class="size-4" />
                  </button>
                {/if}
                <button
                  class="text-muted-foreground hover:text-foreground transition-colors"
                  onclick={(e) => toggleShareMenu(file.key, e)}
                  title="Copy presigned URL"
                >
                  {#if copiedKey === file.key}
                    <Check class="size-4 text-green-500" />
                  {:else}
                    <Share2 class="size-4" />
                  {/if}
                </button>
                <a href={downloadUrl(file.key)} class="text-muted-foreground hover:text-foreground" onclick={(e) => e.stopPropagation()} title="Download">
                  <Download class="size-4" />
                </a>
                <button
                  class="text-muted-foreground hover:text-destructive transition-colors"
                  onclick={(e) => deleteObject(file.key, e)}
                  title="Delete"
                >
                  <Trash2 class="size-4" />
                </button>
              </span>
            </Table.Cell>
          </Table.Row>
          {#if versionKey === file.key}
            <Table.Row>
              <Table.Cell colspan={4} class="p-0">
                <div class="p-2">
                  <VersionHistory
                    {bucket}
                    objectKey={file.key}
                    onClose={() => (versionKey = null)}
                    onVersionDeleted={() => queryClient.invalidateQueries({ queryKey: objectKeys.list(bucket, prefix) })}
                  />
                </div>
              </Table.Cell>
            </Table.Row>
          {/if}
        {/each}
      </Table.Body>
    </Table.Root>
  {/if}
</div>


<Dialog
  open={showCreateFolder}
  title="Create folder"
  description="Create an empty folder marker in the current location."
  loading={createFolderMutation.isPending}
  onClose={() => { showCreateFolder = false; newFolderName = '' }}
>
  <form id="create-folder-form" onsubmit={(e) => { e.preventDefault(); createFolder() }} class="flex flex-col gap-1.5">
    <label for="folder-name" class="text-sm font-medium text-black dark:text-white">Folder name</label>
    <Input
      bind:ref={createFolderInput}
      id="folder-name"
      type="text"
      bind:value={newFolderName}
      placeholder="folder-name"
      class="bg-white dark:bg-base"
      disabled={createFolderMutation.isPending}
    />
  </form>
  {#snippet footer()}
    <Button type="button" variant="default" disabled={createFolderMutation.isPending} onclick={() => { showCreateFolder = false; newFolderName = '' }}>
      Cancel
    </Button>
    <Button type="submit" form="create-folder-form" variant="highlighted" disabled={createFolderMutation.isPending || !newFolderName.trim()}>
      {createFolderMutation.isPending ? 'Creating…' : 'Create folder'}
    </Button>
  {/snippet}
</Dialog>

{#if shareMenuKey}
  <div
    class="fixed z-50 min-w-[8rem] rounded-sm border bg-popover p-1 shadow-md"
    style="top: {shareMenuPos.top}px; left: {shareMenuPos.left}px; transform: translate(-100%, -100%);"
    role="menu"
  >
    {#each expiryOptions as opt}
      <button
        class="w-full rounded-sm px-2 py-1.5 text-left text-sm text-popover-foreground hover:bg-accent hover:text-accent-foreground"
        onclick={() => shareObject(shareMenuKey!, opt.seconds)}
      >
        {opt.label}
      </button>
    {/each}
  </div>
{/if}

{#if pendingDelete}
  <ConfirmDialog
    open
    title={pendingDelete.kind === 'folder' ? 'Delete empty folder?' : 'Delete object?'}
    description={pendingDelete.kind === 'folder'
      ? `This will remove the empty folder marker \"${displayName(pendingDelete.key)}\".`
      : `This will delete \"${displayName(pendingDelete.key)}\" from this bucket.`}
    confirmLabel={pendingDelete.kind === 'folder' ? 'Delete folder' : 'Delete object'}
    confirmVariant="destructive"
    loading={deleteObjectMutation.isPending}
    onClose={() => (pendingDelete = null)}
    onConfirm={confirmPendingDelete}
  />
{/if}
