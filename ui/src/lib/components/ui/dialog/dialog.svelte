<script lang="ts">
  import { Button } from '$lib/components/ui/button'

  type Props = {
    open?: boolean
    title: string
    description?: string
    loading?: boolean
    onClose?: () => void
    children?: import('svelte').Snippet
    footer?: import('svelte').Snippet
  }

  let {
    open = $bindable(false),
    title,
    description,
    loading = false,
    onClose,
    children,
    footer,
  }: Props = $props()

  function close() {
    if (loading) return
    if (onClose) onClose()
    else open = false
  }

  function handleKeydown(event: KeyboardEvent) {
    if (!open) return
    if (event.key === 'Escape') {
      event.preventDefault()
      close()
    }
  }
</script>

<svelte:window onkeydown={handleKeydown} />

{#if open}
  <button
    type="button"
    class="fixed inset-0 z-40 cursor-default bg-black/60"
    aria-label="Close dialog"
    disabled={loading}
    onclick={close}
  ></button>
  <div class="fixed left-1/2 top-1/2 z-50 w-[calc(100vw-2rem)] max-w-lg -translate-x-1/2 -translate-y-1/2">
    <div
      role="dialog"
      aria-modal="true"
      aria-labelledby="dialog-title"
      tabindex="-1"
      class="rounded-sm border border-neutral-200 bg-white p-4 text-black shadow-sm dark:border-coolgray-300 dark:bg-coolgray-100 dark:text-white"
    >
      <div class="flex items-start justify-between gap-4 border-b border-neutral-200 pb-3 dark:border-coolgray-200">
        <div class="flex flex-col gap-1">
          <h2 id="dialog-title" class="text-base font-bold text-black dark:text-white">{title}</h2>
          {#if description}
            <p class="text-sm text-neutral-600 dark:text-neutral-400">{description}</p>
          {/if}
        </div>
        <Button
          type="button"
          variant="ghost"
          size="icon"
          class="shrink-0"
          aria-label="Close dialog"
          disabled={loading}
          onclick={close}
        >
          ×
        </Button>
      </div>

      <div class="mt-4 text-sm text-neutral-700 dark:text-neutral-300">
        {@render children?.()}
      </div>

      <div class="mt-4 flex flex-wrap justify-end gap-2 border-t border-neutral-200 pt-3 dark:border-coolgray-200">
        {#if footer}
          {@render footer()}
        {:else}
          <Button type="button" variant="default" disabled={loading} onclick={close}>Close</Button>
        {/if}
      </div>
    </div>
  </div>
{/if}
