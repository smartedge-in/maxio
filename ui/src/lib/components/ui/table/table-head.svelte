<script lang="ts">
	import { cn, type WithElementRef } from "$lib/utils.js";
	import type { HTMLThAttributes } from "svelte/elements";

	let {
		ref = $bindable(null),
		class: className,
		children,
		...restProps
	}: WithElementRef<HTMLThAttributes> = $props();
</script>

<th
	bind:this={ref}
	data-slot="table-head"
	class={cn(
		"bg-clip-padding px-3 py-2 text-start align-middle text-xs font-bold uppercase tracking-wide text-neutral-600 whitespace-nowrap dark:text-neutral-400 [&:has([role=checkbox])]:pe-0",
		className
	)}
	{...restProps}
>
	{@render children?.()}
</th>
