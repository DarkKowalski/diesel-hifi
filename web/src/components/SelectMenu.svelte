<script lang="ts">
  interface Option { value: string; label: string; detail?: string }
  let { id, label, value, options, disabled = false, dark = false, align = 'left', source, onchange }:
    { id: string; label: string; value: string; options: Option[]; disabled?: boolean; dark?: boolean;
      align?: 'left' | 'right'; source?: string; onchange: (value: string) => void } = $props();
  let root = $state<HTMLDivElement>();
  let open = $state(false);
  let active = $state(0);
  const selected = $derived(options.find((option) => option.value === value));
  function show() {
    if (disabled) return;
    active = Math.max(0, options.findIndex((option) => option.value === value));
    open = true;
  }
  function choose(index: number) {
    const option = options[index];
    if (!option) return;
    open = false;
    onchange(option.value);
  }
  function keydown(event: KeyboardEvent) {
    if (['ArrowDown', 'ArrowUp', 'Home', 'End', 'Enter', ' '].includes(event.key)) {
      event.preventDefault();
      if (!open) { show(); return; }
      if (event.key === 'Enter' || event.key === ' ') choose(active);
      else if (event.key === 'Home') active = 0;
      else if (event.key === 'End') active = options.length - 1;
      else active = (active + (event.key === 'ArrowDown' ? 1 : -1) + options.length) % options.length;
    } else if (event.key === 'Escape' && open) { event.preventDefault(); event.stopPropagation(); open = false; }
    else if (event.key === 'Tab') open = false;
  }
  $effect(() => { if (disabled) open = false; });
</script>

<svelte:window onpointerdown={(event) => { if (root && !root.contains(event.target as Node)) open = false; }} />
<div bind:this={root} class="relative min-w-0" onfocusout={(event) => { if (!root?.contains(event.relatedTarget as Node)) open = false; }}>
  <button type="button" role="combobox" class="w-full justify-between gap-3 rounded-xl border px-3 py-2 text-left font-medium focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand-500 focus-visible:ring-offset-2 {dark ? 'border-white/15 bg-white/5 text-stone-100 hover:bg-white/10' : 'border-stone-200 bg-stone-50 text-stone-800 hover:border-stone-300 hover:bg-stone-100'}" {disabled} aria-label={label} aria-haspopup="listbox" aria-expanded={open} aria-controls={`${id}-options`} aria-activedescendant={open ? `${id}-option-${active}` : undefined} data-testid={id} data-value={value} data-source={source} onclick={() => open ? open = false : show()} onkeydown={keydown}>
    <span class="min-w-0 truncate">{selected?.label ?? label}</span>
    <svg class="size-4 shrink-0 text-stone-400 transition-transform duration-150 {open ? 'rotate-180' : ''}" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" aria-hidden="true"><path d="m6 9 6 6 6-6" /></svg>
  </button>
  {#if open}
    <div id={`${id}-options`} role="listbox" aria-label={label} class="absolute top-[calc(100%+0.5rem)] z-50 grid min-w-full max-w-[calc(100vw-2rem)] gap-1 rounded-xl border border-stone-200 bg-white p-1.5 text-stone-900 shadow-xl {align === 'right' ? 'right-0 w-max' : 'left-0 w-full'}">
      {#each options as option, index (option.value)}
        <button id={`${id}-option-${index}`} type="button" role="option" tabindex="-1" aria-selected={option.value === value} data-value={option.value} class="min-h-12 w-full justify-between gap-4 rounded-lg px-3 text-left {active === index ? 'bg-brand-50 text-brand-700' : 'hover:bg-stone-100'}" onpointermove={() => active = index} onpointerdown={(event) => event.preventDefault()} onclick={() => choose(index)}>
          <span class="grid min-w-0 gap-1"><span class="break-words text-sm">{option.label}</span>{#if option.detail}<span class="text-[11px] font-normal text-stone-500">{option.detail}</span>{/if}</span>
          {#if option.value === value}<svg class="size-4 shrink-0 text-brand-600" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true"><path d="m5 12 4 4L19 6" /></svg>{/if}
        </button>
      {/each}
    </div>
  {/if}
</div>
