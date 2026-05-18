<script lang="ts">
	import { cn, type WithElementRef } from "$lib/utils.js";
	import type { HTMLAttributes } from "svelte/elements";

	let {
		ref = $bindable(null),
		class: className,
		children,
		...restProps
	}: WithElementRef<HTMLAttributes<HTMLTableRowElement>> = $props();
</script>

<tr
	bind:this={ref}
	data-slot="table-row"
	class={cn(
		"border-b border-neutral-200 dark:border-coolgray-200 transition-colors hover:bg-neutral-100 dark:hover:bg-coolgray-200 data-[state=selected]:bg-neutral-100 dark:data-[state=selected]:bg-coolgray-200",
		className
	)}
	{...restProps}
>
	{@render children?.()}
</tr>
