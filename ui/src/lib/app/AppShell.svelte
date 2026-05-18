<script lang="ts">
  import { base } from "$app/paths";
  import { onMount } from "svelte";
  import { createMutation, createQuery } from "@tanstack/svelte-query";
  import Login from "$lib/Login.svelte";
  import BucketList from "$lib/BucketList.svelte";
  import ObjectBrowser from "$lib/ObjectBrowser.svelte";
  import BucketSettings from "$lib/BucketSettings.svelte";
  import Home from "lucide-svelte/icons/home";
  import LogOut from "lucide-svelte/icons/log-out";

  import ArrowLeft from "lucide-svelte/icons/arrow-left";
  import ChevronRight from "lucide-svelte/icons/chevron-right";
  import Sun from "lucide-svelte/icons/sun";
  import Moon from "lucide-svelte/icons/moon";
  import Monitor from "lucide-svelte/icons/monitor";
  import { Sonner } from "$lib/components/ui/sonner";
  import { checkAuth, logout } from "$lib/api/auth";
  import { authKeys } from "$lib/api/keys";
  import { queryClient } from "$lib/query/client";

  type ThemeMode = "light" | "system" | "dark";

  const authQuery = createQuery(() => ({
    queryKey: authKeys.check(),
    queryFn: checkAuth,
    retry: false,
  }));
  const logoutMutation = createMutation(() => ({
    mutationFn: logout,
    onSuccess: () => {
      queryClient.clear();
      queryClient.invalidateQueries({ queryKey: authKeys.all });
    },
  }));

  let authenticatedOverride = $state<boolean | null>(null);
  let collapsed = $state(false);
  let selectedBucket = $state<string | null>(null);
  let currentView = $state<"objects" | "settings">("objects");
  let objectBrowserRef = $state<ObjectBrowser | null>(null);
  let currentPrefix = $state("");
  let currentBreadcrumbs = $state<{ label: string; prefix: string }[]>([]);
  let themeMode = $state<ThemeMode>("system");
  let isDark = $state(true);
  let pendingPrefix = $state<string | null>(null);

  const themeOptions: { mode: ThemeMode; label: string; icon: typeof Sun }[] = [
    { mode: "light", label: "Light", icon: Sun },
    { mode: "system", label: "System", icon: Monitor },
    { mode: "dark", label: "Dark", icon: Moon },
  ];

  $effect(() => {
    if (objectBrowserRef && pendingPrefix) {
      objectBrowserRef.navigateTo(pendingPrefix);
      pendingPrefix = null;
    }
  });

  function applyHash() {
    const hash = window.location.hash.slice(1) || "/";
    if (hash === "/") {
      selectedBucket = null;
      currentView = "objects";
      currentPrefix = "";
      currentBreadcrumbs = [];
    } else {
      const parts = hash.slice(1).split("/"); // remove leading /
      const bucket = decodeURIComponent(parts[0]);
      const rest = parts.slice(1).join("/");
      selectedBucket = bucket;
      if (rest === "settings") {
        currentView = "settings";
        currentPrefix = "";
        currentBreadcrumbs = [];
      } else {
        currentView = "objects";
        if (rest) {
          if (objectBrowserRef) {
            objectBrowserRef.navigateTo(rest);
          } else {
            pendingPrefix = rest;
          }
        }
      }
    }
  }

  function updateHash() {
    if (!selectedBucket) {
      window.location.hash = "/";
    } else if (currentPrefix) {
      window.location.hash = `/${encodeURIComponent(selectedBucket)}/${currentPrefix}`;
    } else {
      window.location.hash = `/${encodeURIComponent(selectedBucket)}`;
    }
  }

  onMount(() => {
    collapsed = localStorage.getItem("sidebar-collapsed") === "true";
    const savedTheme = localStorage.getItem("theme");
    themeMode = isThemeMode(savedTheme) ? savedTheme : "system";
    applyTheme(themeMode, false);

    const mediaQuery = window.matchMedia("(prefers-color-scheme: dark)");
    const handleSystemThemeChange = () => {
      if (themeMode === "system") {
        applyTheme("system", false);
      }
    };
    mediaQuery.addEventListener("change", handleSystemThemeChange);

    window.addEventListener("hashchange", applyHash);
    if (window.location.hash && window.location.hash !== "#/") {
      applyHash();
    }

    return () => {
      window.removeEventListener("hashchange", applyHash);
      mediaQuery.removeEventListener("change", handleSystemThemeChange);
    };
  });

  function handleLogin() {
    authenticatedOverride = true;
    queryClient.invalidateQueries({ queryKey: authKeys.all });
  }

  async function handleLogout() {
    await logoutMutation.mutateAsync();
    authenticatedOverride = false;
    selectedBucket = null;
    currentView = "objects";
    currentPrefix = "";
    currentBreadcrumbs = [];
  }

  function isThemeMode(value: string | null): value is ThemeMode {
    return value === "light" || value === "system" || value === "dark";
  }

  function applyTheme(mode: ThemeMode, persist = true) {
    themeMode = mode;
    const systemDark = window.matchMedia("(prefers-color-scheme: dark)").matches;
    isDark = mode === "dark" || (mode === "system" && systemDark);
    document.documentElement.classList.toggle("dark", isDark);
    if (persist) {
      localStorage.setItem("theme", mode);
    }
  }

  function cycleTheme() {
    const index = themeOptions.findIndex((option) => option.mode === themeMode);
    const next = themeOptions[(index + 1) % themeOptions.length]?.mode ?? "system";
    applyTheme(next);
  }

  function currentThemeLabel() {
    return themeOptions.find((option) => option.mode === themeMode)?.label ?? "System";
  }

  function selectBucket(name: string) {
    selectedBucket = name;
    currentView = "objects";
    currentPrefix = "";
    currentBreadcrumbs = [];
    updateHash();
  }

  function goToSettings(name: string) {
    selectedBucket = name;
    currentView = "settings";
    currentPrefix = "";
    currentBreadcrumbs = [];
    window.location.hash = `/${encodeURIComponent(name)}/settings`;
  }

  function goHome() {
    selectedBucket = null;
    currentView = "objects";
    currentPrefix = "";
    currentBreadcrumbs = [];
    updateHash();
  }

  function handlePrefixChange(p: string, crumbs: { label: string; prefix: string }[]) {
    currentPrefix = p;
    currentBreadcrumbs = crumbs;
    updateHash();
  }
</script>

{#if authQuery.isPending && authenticatedOverride === null}
  <!-- loading -->
{:else if !(authenticatedOverride ?? authQuery.isSuccess)}
  <Login onLogin={handleLogin} />
{:else}
  <div class="relative flex h-screen bg-background">
    <nav
      class="relative flex flex-col border-r bg-sidebar-background transition-[width] duration-200"
      class:w-64={!collapsed}
      class:w-16={collapsed}
      style="border-color: var(--cool-sidebar-border);"
    >
      <!-- Collapse/expand toggle -->
      <button
        onclick={() => { collapsed = !collapsed; localStorage.setItem("sidebar-collapsed", String(collapsed)); }}
        class="absolute top-8 -right-3 z-10 flex size-6 items-center justify-center rounded-full border bg-card text-muted-foreground shadow-sm transition-colors hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-coollabs dark:focus-visible:ring-warning focus-visible:ring-offset-2 dark:focus-visible:ring-offset-base"
        style="border-color: var(--cool-sidebar-border);"
        title={collapsed ? "Expand sidebar" : "Collapse sidebar"}
        aria-label={collapsed ? "Expand sidebar" : "Collapse sidebar"}
        aria-expanded={!collapsed}
      >
        <svg
          class="size-3.5 transition-transform"
          class:rotate-180={collapsed}
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          stroke-width="2.2"
          stroke-linecap="round"
          stroke-linejoin="round"
          aria-hidden="true"
        >
          <path d="M15 18 9 12l6-6" />
        </svg>
      </button>

      <!-- Logo -->
      <div
        class="flex h-14 items-center overflow-hidden"
        class:px-4={!collapsed}
        class:justify-center={collapsed}
      >
        <img src={`${base}/maxio.png`} alt="MaxIO" class="size-6 shrink-0" />
        {#if !collapsed}
          <span
            class="ml-2 text-2xl font-bold tracking-tight text-foreground whitespace-nowrap"
            >MaxIO</span
          >
        {/if}
      </div>

      <!-- Nav items -->
      <div class="flex flex-1 flex-col gap-0.5 p-2">
        <button
          onclick={goHome}
          class="flex min-h-7 w-full items-center rounded-sm py-1 text-left text-sm font-medium transition-colors overflow-hidden bg-neutral-200 text-black dark:bg-coolgray-200 dark:text-warning hover:bg-neutral-300 dark:hover:bg-coolgray-100"
          class:gap-3={!collapsed}
          class:px-2={!collapsed}
          class:justify-center={collapsed}
          class:size-8={collapsed}
          title="Buckets"
        >
          <Home class="size-4 shrink-0" />
          {#if !collapsed}<span class="whitespace-nowrap">Buckets</span>{/if}
        </button>
      </div>

      <!-- Bottom: theme toggle + logout -->
      <div
        class="flex flex-col gap-0.5 p-2"
      >
        {#if collapsed}
          <button
            onclick={cycleTheme}
            class="mx-auto flex size-8 items-center justify-center rounded-sm text-sm font-medium text-muted-foreground transition-colors hover:bg-muted hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-coollabs dark:focus-visible:ring-warning"
            aria-label={`Theme: ${currentThemeLabel()}. Click to switch theme.`}
            title={`Theme: ${currentThemeLabel()}`}
          >
            {#if themeMode === "light"}
              <Sun class="size-4 shrink-0" />
            {:else if themeMode === "system"}
              <Monitor class="size-4 shrink-0" />
            {:else}
              <Moon class="size-4 shrink-0" />
            {/if}
          </button>
        {:else}
          <div class="flex min-h-7 w-full items-center justify-between gap-3 rounded-sm px-2 py-1 text-sm text-muted-foreground">
            <span class="whitespace-nowrap">Theme</span>
            <div
              class="inline-flex items-center gap-0.5 rounded-sm bg-neutral-100 p-0.5 dark:bg-coolgray-200"
              aria-label="Theme"
            >
              {#each themeOptions as option}
                {@const Icon = option.icon}
                <button
                  type="button"
                  onclick={() => applyTheme(option.mode)}
                  class={`grid size-6 place-items-center rounded-sm text-neutral-500 transition-colors hover:text-black focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-coollabs dark:text-neutral-400 dark:hover:text-white dark:focus-visible:ring-warning ${themeMode === option.mode ? 'bg-white text-coollabs shadow-sm dark:bg-base dark:text-warning' : ''}`}
                  aria-label={`Use ${option.label} theme`}
                  aria-pressed={themeMode === option.mode}
                  title={option.label}
                >
                  <Icon class="size-4" />
                </button>
              {/each}
            </div>
          </div>
        {/if}
        <button
          onclick={handleLogout}
          class="flex min-h-7 w-full items-center rounded-sm py-1 text-left text-sm font-medium text-muted-foreground transition-colors hover:bg-muted overflow-hidden"
          class:gap-3={!collapsed}
          class:px-2={!collapsed}
          class:justify-center={collapsed}
          class:size-8={collapsed}
          aria-label="Sign out"
          title="Sign out"
        >
          <LogOut class="size-4 shrink-0" />
          {#if !collapsed}<span class="whitespace-nowrap">Sign out</span>{/if}
        </button>
      </div>
    </nav>

    <main class="flex flex-1 flex-col overflow-hidden">
      <!-- Header bar -->
      <div
        class="flex h-14 shrink-0 items-center gap-2 px-6"
      >
        {#if selectedBucket}
          <button
            type="button"
            onclick={() => {
              if (currentView === "settings") {
                goHome();
              } else {
                objectBrowserRef?.goUp();
              }
            }}
            class="shrink-0 rounded-sm p-1 text-neutral-600 transition-colors hover:text-coollabs focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-coollabs dark:text-neutral-400 dark:hover:text-warning dark:focus-visible:ring-warning"
            aria-label={currentView === "settings" ? "Back to buckets" : "Go up one folder"}
          >
            <ArrowLeft class="size-4" />
          </button>
          <nav aria-label="Breadcrumb" class="min-w-0 overflow-x-auto">
            <ol class="flex flex-wrap items-center gap-1.5 text-sm font-medium">
              <li class="inline-flex items-center gap-1.5">
                <button
                  type="button"
                  class="shrink-0 rounded-sm text-neutral-600 transition-colors hover:text-coollabs focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-coollabs dark:text-neutral-400 dark:hover:text-warning dark:focus-visible:ring-warning"
                  onclick={goHome}>Buckets</button
                >
              </li>
              <li class="inline-flex items-center gap-1.5 text-neutral-400" aria-hidden="true">
                <ChevronRight class="size-3 shrink-0" />
              </li>
              {#if currentView === "settings"}
                <li class="inline-flex items-center gap-1.5">
                  <button
                    type="button"
                    class="shrink-0 rounded-sm text-neutral-600 transition-colors hover:text-coollabs focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-coollabs dark:text-neutral-400 dark:hover:text-warning dark:focus-visible:ring-warning"
                    onclick={() => selectBucket(selectedBucket!)}
                  >{selectedBucket}</button>
                </li>
                <li class="inline-flex items-center gap-1.5 text-neutral-400" aria-hidden="true">
                  <ChevronRight class="size-3 shrink-0" />
                </li>
                <li class="inline-flex items-center gap-1.5">
                  <span class="shrink-0 text-black dark:text-white" aria-current="page">Settings</span>
                </li>
              {:else if currentBreadcrumbs.length > 1}
                {#each currentBreadcrumbs as crumb, i}
                  {#if i < currentBreadcrumbs.length - 1}
                    <li class="inline-flex items-center gap-1.5">
                      <button
                        type="button"
                        class="shrink-0 rounded-sm text-neutral-600 transition-colors hover:text-coollabs focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-coollabs dark:text-neutral-400 dark:hover:text-warning dark:focus-visible:ring-warning"
                        onclick={() => objectBrowserRef?.navigateTo(crumb.prefix)}
                      >{crumb.label}</button>
                    </li>
                    <li class="inline-flex items-center gap-1.5 text-neutral-400" aria-hidden="true">
                      <ChevronRight class="size-3 shrink-0" />
                    </li>
                  {:else}
                    <li class="inline-flex items-center gap-1.5">
                      <span class="shrink-0 text-black dark:text-white" aria-current="page">{crumb.label}</span>
                    </li>
                  {/if}
                {/each}
              {:else}
                <li class="inline-flex items-center gap-1.5">
                  <span class="shrink-0 text-black dark:text-white" aria-current="page">{selectedBucket}</span>
                </li>
              {/if}
            </ol>
          </nav>
        {:else}
          <h2 class="text-lg font-semibold">Buckets</h2>
        {/if}
      </div>
      <!-- Scrollable content -->
      <div class="flex-1 overflow-auto p-6">
        {#if selectedBucket && currentView === "settings"}
          <BucketSettings
            bucket={selectedBucket}
            onBack={() => selectBucket(selectedBucket!)}
          />
        {:else if selectedBucket}
          <ObjectBrowser
            bind:this={objectBrowserRef}
            bucket={selectedBucket}
            onBack={goHome}
            onPrefixChange={handlePrefixChange}
          />
        {:else}
          <BucketList onSelect={selectBucket} onSettings={goToSettings} />
        {/if}
      </div>
    </main>
  </div>
  <Sonner theme={isDark ? 'dark' : 'light'} />
{/if}
