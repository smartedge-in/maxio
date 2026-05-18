<script lang="ts">
	import type { HTMLTableAttributes } from "svelte/elements";
	import { cn, type WithElementRef } from "$lib/utils.js";

	let {
		ref = $bindable(null),
		class: className,
		children,
		...restProps
	}: WithElementRef<HTMLTableAttributes> = $props();
</script>

<div data-slot="table-container" class="relative w-full overflow-x-auto rounded-sm border border-neutral-200 dark:border-coolgray-300">
	<table
		bind:this={ref}
		data-slot="table"
		class={cn(
			"w-full min-w-[42rem] border-collapse caption-bottom text-left text-sm",
			className
		)}
		{...restProps}
	>
		{@render children?.()}
	</table>
</div>
