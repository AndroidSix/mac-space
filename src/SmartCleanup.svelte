<script lang="ts">
  import { onMount } from 'svelte';
  import { Check, CircleHelp, LoaderCircle, ShieldCheck, Sparkles, X } from '@lucide/svelte';
  import { cleanupBlock, cleanupImpact, recommendedIds } from './smart-cleanup';
  import type { BatchReport, CleanupProgress, Entry } from './types';

  let {
    entries,
    ready,
    scanning,
    cleaning,
    stopping,
    progress,
    scanDetail,
    report,
    error,
    t,
    bytes,
    onClean,
    onClose,
    onStop,
    onRetry,
  }: {
    entries: Entry[];
    ready: boolean;
    scanning: boolean;
    cleaning: boolean;
    stopping: boolean;
    progress: CleanupProgress | null;
    scanDetail: string;
    report: BatchReport | null;
    error: string | null;
    t: (key: string, values?: Record<string, string | number>) => string;
    bytes: (value: number | undefined, signed?: boolean) => string;
    onClean: (ids: string[]) => void;
    onClose: () => void;
    onStop: () => void;
    onRetry: () => void;
  } = $props();

  let dialog: HTMLDialogElement;
  let selected = $state<string[]>([]);
  const eligible = $derived(entries.filter((entry) => !cleanupBlock(entry)));
  const recommended = $derived(eligible.filter((entry) => cleanupImpact(entry)?.recommended));
  const optional = $derived(eligible.filter((entry) => !cleanupImpact(entry)?.recommended));
  const unavailable = $derived(entries.filter((entry) => cleanupBlock(entry)));
  const selectedEntries = $derived(eligible.filter((entry) => selected.includes(entry.id)));
  const selectedBytes = $derived(selectedEntries.reduce((sum, entry) => sum + entry.bytes, 0));
  const successCount = $derived(report?.items.filter((item) => !item.error).length ?? 0);
  const failedCount = $derived(report?.items.filter((item) => item.error).length ?? 0);

  $effect(() => {
    selected = ready ? recommendedIds(entries) : [];
  });
  onMount(() => {
    dialog.showModal();
  });

  function close() {
    if (cleaning) return;
    dialog.close();
    onClose();
  }
  function toggle(id: string) {
    selected = selected.includes(id) ? selected.filter((value) => value !== id) : [...selected, id];
  }
  function errorText(value: string) {
    return t(value.split(':')[0]);
  }
</script>

<dialog
  bind:this={dialog}
  class="smart-dialog"
  aria-labelledby="smart-title"
  aria-describedby="smart-description"
  oncancel={(event) => {
    event.preventDefault();
    close();
  }}
>
  <div class="smart-layout">
    <header class="smart-header">
      <div class="smart-heading">
        <span class="smart-emblem"><Sparkles size={23} /></span>
        <div>
          <h2 id="smart-title">{t(report ? 'smartDone' : 'smartTitle')}</h2>
          <p id="smart-description">{t('smartIntro')}</p>
        </div>
      </div>
      <button class="icon-button" disabled={cleaning} aria-label={t('dismiss')} onclick={close}
        ><X size={19} /></button
      >
    </header>

    <div class="smart-body" aria-busy={scanning || cleaning}>
      {#if error}<div class="notice error" role="alert">{error}</div>{/if}
      {#if scanning}
        <div class="smart-scanning" role="status">
          <LoaderCircle size={28} class="spin" />
          <h3>{t('smartScanning')}</h3>
          <p>{t('smartScanningHint')}</p>
          <code class="smart-scan-path">{scanDetail}</code>
        </div>
      {:else if report}
        <div class="smart-result-summary" role="status">
          <h3>
            {t('smartDoneSummary', {
              success: successCount,
              failed: failedCount,
              skipped: selected.length - report.items.length,
            })}
          </h3>
          <p>{t('smartSpaceChange', { size: bytes(report.availableChange, true) })}</p>
        </div>
        {#if report.persistenceError}<div class="notice error" role="alert">
            {errorText(report.persistenceError)}
          </div>{/if}
        <div class="smart-results">
          {#each entries.filter((entry) => selected.includes(entry.id)) as entry}
            {@const result = report.items.find((item) => item.id === entry.id)}
            <div class="smart-result-row">
              <div>
                <strong>{t(entry.id)}</strong>{#if result?.error}<p>
                    {errorText(result.error)}
                  </p>{/if}
              </div>
              <span class="status-badge" class:warning={!!result?.error || !result}>
                {#if result && !result.error}<Check size={13} />{/if}{t(
                  !result ? 'smartNotRun' : result.error ? 'smartFailed' : 'smartSucceeded',
                )}
              </span>
            </div>
          {/each}
        </div>
      {:else if cleaning}
        <div class="smart-scanning" role="status">
          <LoaderCircle size={28} class="spin" />
          <h3>{progress ? t('smartRunning', { name: t(progress.id) }) : t('working')}</h3>
          <p>
            {t('smartProgress', {
              completed: progress?.completed ?? 0,
              total: progress?.total ?? selected.length,
            })}
          </p>
          <progress
            max={progress?.total ?? selected.length}
            value={progress?.completed ?? 0}
            aria-label={t('smartClean')}
          ></progress>
          <p>{t('smartStopHint')}</p>
        </div>
      {:else if ready}
        <div class="smart-selection-tools">
          <span aria-live="polite"
            >{t('smartSelection', {
              count: selectedEntries.length,
              size: bytes(selectedBytes),
            })}</span
          >
          <div>
            <button class="text-button" onclick={() => (selected = recommendedIds(entries))}
              >{t('smartSelectRecommended')}</button
            >
            <button class="text-button" onclick={() => (selected = [])}
              >{t('smartSelectNone')}</button
            >
          </div>
        </div>
        {#if !entries.length}<p class="smart-empty">{t('smartEmpty')}</p>
        {:else}
          {#if !recommended.length}<p class="smart-empty">{t('smartNoRecommended')}</p>{/if}
          {#each [{ title: 'smartRecommended', items: recommended }, { title: 'smartOptional', items: optional }, { title: 'smartUnavailable', items: unavailable }] as group}
            {#if group.items.length}<section class="smart-group" aria-label={t(group.title)}>
                <h3>{t(group.title)}<span>{group.items.length}</span></h3>
                {#each group.items as entry}
                  {@const block = cleanupBlock(entry)}
                  <label class="smart-candidate" class:unavailable={!!block}>
                    <input
                      type="checkbox"
                      checked={selected.includes(entry.id)}
                      disabled={!!block}
                      onchange={() => toggle(entry.id)}
                    />
                    <span class="smart-candidate-copy"
                      ><strong>{t(entry.id)}</strong>
                      <span>{t(cleanupImpact(entry)?.hint ?? 'invalid_target')}</span>
                      {#if block}<span class="smart-block-reason"
                          >{t(block)}{#if entry.blocked.length}
                            {entry.blocked.join(', ')}{/if}</span
                        >{/if}
                      <code title={entry.path}>{entry.path.replace(/^\/Users\/[^/]+/, '~')}</code>
                    </span>
                    <span class="smart-candidate-size">{bytes(entry.bytes)}</span>
                  </label>
                {/each}
              </section>{/if}
          {/each}
        {/if}
      {/if}
    </div>

    <footer class="smart-footer">
      {#if !report && !cleaning && ready}<p class="smart-warning">
          <ShieldCheck size={16} /><span>{t('smartWarning')}</span>
        </p>{/if}
      <div class="smart-footer-actions">
        <span class="smart-footer-note"><CircleHelp size={14} />{t('localOnly')}</span>
        <div class="smart-action-buttons">
          {#if cleaning}<button class="secondary" disabled={stopping} onclick={onStop}
              >{t(stopping ? 'smartStopping' : 'smartStop')}</button
            >
          {:else if report}<button class="primary" onclick={close}>{t('smartClose')}</button>
          {:else}<button class="secondary" onclick={close}>{t('cancel')}</button>
            {#if ready}<button
                class="primary"
                disabled={!selectedEntries.length}
                onclick={() => onClean(selectedEntries.map((entry) => entry.id))}
                >{t('smartDelete', { size: bytes(selectedBytes) })}</button
              >
            {:else if !scanning}<button class="primary" onclick={onRetry}>{t('smartRetry')}</button
              >{/if}
          {/if}
        </div>
      </div>
    </footer>
  </div>
</dialog>
