<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { Leaf, LoaderCircle, RefreshCw } from '@lucide/svelte';
  import type { PowerStatus } from './types';
  let {
    status,
    native,
    t,
    onChanged,
  }: {
    status: PowerStatus | undefined;
    native: boolean;
    t: (key: string) => string;
    onChanged: (status: PowerStatus) => void;
  } = $props();
  const current = $derived(status?.current ?? '');
  let selected = $state('');
  let working = $state(false);
  let message = $state('');
  let failed = $state(false);
  $effect(() => {
    selected = current;
  });
  async function update(apply = false) {
    if (working) return;
    working = true;
    message = '';
    try {
      const result = await invoke<PowerStatus>(
        apply ? 'set_power_mode' : 'get_power_status',
        apply ? { id: selected } : {},
      );
      onChanged(result);
      message = t(apply ? 'powerApplied' : 'powerRefreshed');
      failed = false;
    } catch (error) {
      message = t(String(error).split(':')[0]);
      failed = true;
      selected = current;
    } finally {
      working = false;
    }
  }
</script>

<section class="panel power-panel" aria-labelledby="power-title" aria-busy={working}>
  <div class="section-heading">
    <div>
      <h2 id="power-title"><Leaf size={18} />{t('powerControl')}</h2>
      <p class="small-note">{t('powerHint')}</p>
    </div>
    <button
      class="icon-button"
      disabled={!native || working}
      aria-label={t('refreshPower')}
      onclick={() => update()}><RefreshCw size={16} /></button
    >
  </div>
  <div class="power-controls">
    <label for="power-mode">{t('powerMode')}</label>
    <select
      id="power-mode"
      bind:value={selected}
      disabled={!native || working || !status?.modes.length}
    >
      {#if !status?.modes.length}<option value="">{t('power_unavailable')}</option>{/if}
      {#each status?.modes ?? [] as mode}<option value={mode.id}>{t(mode.name)}</option>{/each}
    </select>
    <button
      class="primary"
      disabled={!native || working || !selected || selected === current}
      onclick={() => update(true)}
      >{#if working}<LoaderCircle size={16} class="spin" />{/if}{t(
        working ? 'working' : 'applyPower',
      )}</button
    >
  </div>
  <p class="small-note">{t(status?.requiresAdmin ? 'powerMacNote' : 'powerWindowsNote')}</p>
  <p class="power-feedback" class:error-text={failed} role="status">{message}</p>
</section>
