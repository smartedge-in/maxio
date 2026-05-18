<script lang="ts">
  import { Button, type ButtonVariant } from '$lib/components/ui/button'
  import { Input } from '$lib/components/ui/input'

  type Props = {
    open?: boolean
    title: string
    description?: string
    confirmLabel?: string
    cancelLabel?: string
    confirmVariant?: ButtonVariant
    confirmationText?: string
    confirmationLabel?: string
    loading?: boolean
    onClose?: () => void
    onConfirm: () => void | Promise<void>
  }

  let {
    open = $bindable(false),
    title,
    description,
    confirmLabel = 'Confirm',
    cancelLabel = 'Cancel',
    confirmVariant = 'highlighted',
    confirmationText,
    confirmationLabel,
    loading = false,
    onClose,
    onConfirm,
  }: Props = $props()

  let typedConfirmation = $state('')
  let confirmationInput = $state<HTMLInputElement | null>(null)
  let cancelButton = $state<HTMLButtonElement | null>(null)
  const canConfirm = $derived(!confirmationText || typedConfirmation === confirmationText)
  const closeLabel = $derived(
    confirmVariant === 'destructive' || confirmationText
      ? 'Close destructive confirmation'
      : 'Close confirmation'
  )

  $effect(() => {
    if (!open) typedConfirmation = ''
  })

  $effect(() => {
    if (open) {
      queueMicrotask(() => {
        if (confirmationText) confirmationInput?.focus()
        else cancelButton?.focus()
      })
    }
  })

  function close() {
    if (loading) return
    if (onClose) onClose()
    else open = false
  }

  async function confirm() {
    if (!canConfirm || loading) return
    await onConfirm()
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
      aria-labelledby="confirm-dialog-title"
      tabindex="-1"
      class="rounded-sm border border-neutral-200 bg-white p-4 text-black shadow-sm dark:border-coolgray-300 dark:bg-coolgray-100 dark:text-white"
    >
      <div class="flex items-start justify-between gap-4 border-b border-neutral-200 pb-3 dark:border-coolgray-200">
        <div class="flex min-w-0 flex-col gap-1">
          <h2 id="confirm-dialog-title" class="text-base font-bold text-black dark:text-white">{title}</h2>
          {#if description}
            <p class="text-sm text-neutral-600 dark:text-neutral-400">{description}</p>
          {/if}
        </div>
        <Button
          type="button"
          variant="ghost"
          size="icon"
          class="shrink-0"
          aria-label={closeLabel}
          disabled={loading}
          onclick={close}
        >
          ×
        </Button>
      </div>

      {#if confirmationText}
        <div class="mt-4 rounded-sm border border-red-300 bg-red-50 p-3 text-sm text-red-800 dark:border-red-800 dark:bg-red-900/30 dark:text-red-300">
          Type <span class="font-mono font-bold">{confirmationText}</span> to confirm this destructive action.
        </div>
        <label class="mt-3 flex flex-col gap-1.5 text-sm font-medium text-black dark:text-white">
          {confirmationLabel ?? 'Confirmation'}
          <Input
            bind:ref={confirmationInput}
            class="bg-white dark:bg-base"
            bind:value={typedConfirmation}
            autocomplete="off"
            disabled={loading}
          />
        </label>
      {/if}

      <div class="mt-4 flex flex-wrap justify-end gap-2 border-t border-neutral-200 pt-3 dark:border-coolgray-200">
        <Button bind:ref={cancelButton} type="button" variant="default" disabled={loading} onclick={close}>{cancelLabel}</Button>
        <Button type="button" variant={confirmVariant} disabled={loading || !canConfirm} onclick={confirm}>
          {loading ? 'Working…' : confirmLabel}
        </Button>
      </div>
    </div>
  </div>
{/if}
