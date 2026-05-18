<script lang="ts">
  import { Button } from "$lib/components/ui/button";
  import { Input } from "$lib/components/ui/input";
  import { Callout } from "$lib/components/ui/callout";
  import { Highlighted } from "$lib/components/ui/highlighted";
  import { createMutation } from "@tanstack/svelte-query";
  import Eye from "lucide-svelte/icons/eye";
  import EyeOff from "lucide-svelte/icons/eye-off";
  import { login } from "$lib/api/auth";
  import { ApiError } from "$lib/api/http";

  let accessKey = $state('')
  let secretKey = $state('')
  let error = $state('')
  let showSecret = $state(false)

  const loginMutation = createMutation(() => ({
    mutationFn: login,
    onSuccess: () => onLogin(),
  }))

  interface Props {
    onLogin: () => void
  }
  let { onLogin }: Props = $props()

  async function handleSubmit(e: Event) {
    e.preventDefault()
    error = ''
    try {
      await loginMutation.mutateAsync({ accessKey, secretKey })
    } catch (err) {
      console.error('Login failed:', err)
      error = err instanceof ApiError ? err.message : 'Connection failed'
    }
  }
</script>

<div class="flex min-h-screen w-full items-center justify-center bg-gray-50 px-6 py-8 dark:bg-base">
  <div class="mx-auto w-full max-w-md space-y-8 text-black dark:text-white">
    <!-- Title -->
    <h1 class="text-center text-5xl font-extrabold tracking-tight text-gray-900 dark:text-white">MaxIO</h1>

    <form onsubmit={handleSubmit} class="flex flex-col gap-4">
      <!-- Access Key -->
      <div class="flex flex-col gap-1.5">
        <label for="accessKey" class="text-sm text-muted-foreground">
          Access Key <Highlighted>*</Highlighted>
        </label>
        <Input
          id="accessKey"
          type="text"
          bind:value={accessKey}
          autocomplete="username"
          required
        />
      </div>

      <!-- Secret Key -->
      <div class="flex flex-col gap-1.5">
        <label for="secretKey" class="text-sm text-muted-foreground">
          Secret Key <Highlighted>*</Highlighted>
        </label>
        <div class="relative">
          <Input
            id="secretKey"
            type={showSecret ? 'text' : 'password'}
            bind:value={secretKey}
            autocomplete="current-password"
            class="pr-10"
            required
          />
          <button
            type="button"
            onclick={() => showSecret = !showSecret}
            class="absolute right-2 top-1/2 -translate-y-1/2 p-1 text-muted-foreground transition-colors hover:text-foreground"
          >
            {#if showSecret}
              <EyeOff class="size-4" />
            {:else}
              <Eye class="size-4" />
            {/if}
          </button>
        </div>
      </div>

      {#if error}
        <Callout type="danger">{error}</Callout>
      {/if}

      <!-- Login button — large highlighted style -->
      <Button type="submit" variant="highlighted" class="mt-2 h-12 w-full justify-center px-4" disabled={loginMutation.isPending}>
        {loginMutation.isPending ? 'Signing in...' : 'Login'}
      </Button>
    </form>
  </div>
</div>
