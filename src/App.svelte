<script lang="ts">
  import { onMount } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { listen } from '@tauri-apps/api/event';
  import {
    Activity,
    ArrowDown,
    ArrowDownRight,
    ArrowRight,
    Check,
    ChevronRight,
    CircleHelp,
    Cpu,
    Database,
    File,
    Folder,
    HardDrive,
    Languages,
    LayoutDashboard,
    Leaf,
    LoaderCircle,
    MemoryStick,
    Pause,
    Play,
    RefreshCw,
    Search,
    Settings2,
    ShieldCheck,
    Square,
    Sparkles,
    Thermometer,
    Trash2,
    X,
  } from '@lucide/svelte';
  import { messages, type MessageKey } from './i18n';
  import SmartCleanup from './SmartCleanup.svelte';
  import type {
    BatchReport,
    CleanupProgress,
    Entry,
    Language,
    Metrics,
    ProcessInfo,
    Progress,
    Snapshot,
  } from './types';

  type Page = 'overview' | 'cleanup' | 'analysis' | 'monitor' | 'settings';
  type Confirmation =
    | { kind: 'cache'; entry: Entry }
    | { kind: 'process'; process: ProcessInfo; action: 'lower' | 'terminate' };
  function readPrefs(): {
    language: Language;
    interval: number;
    economy: boolean;
    autoRefresh: boolean;
  } {
    try {
      const value = JSON.parse(localStorage.getItem('macspace.preferences') ?? '{}');
      return {
        language: ['system', 'zh', 'en'].includes(value.language) ? value.language : 'system',
        interval: [3000, 5000, 10000].includes(value.interval) ? value.interval : 5000,
        economy: value.economy !== false,
        autoRefresh: value.autoRefresh !== false,
      };
    } catch {
      return { language: 'system', interval: 5000, economy: true, autoRefresh: true };
    }
  }
  const prefs = readPrefs();
  const native = '__TAURI_INTERNALS__' in window;
  let language = $state<Language>(prefs.language);
  let systemLanguage = $state<'zh' | 'en'>(navigator.language.startsWith('zh') ? 'zh' : 'en');
  const locale = $derived(language === 'system' ? systemLanguage : language);
  const copy = $derived(messages[locale]);
  function t(key: MessageKey | string, values: Record<string, string | number> = {}): string {
    let value = copy[key as MessageKey] ?? key;
    for (const [name, replacement] of Object.entries(values))
      value = value.replaceAll(`{${name}}`, String(replacement));
    return value;
  }
  let page = $state<Page>('overview');
  let interval = $state(prefs.interval);
  let economy = $state(prefs.economy);
  let autoRefresh = $state(prefs.autoRefresh);
  let cpuHistory = $state<number[]>([]);
  let lastIncremental = 0;
  let paused = $state(false);
  let focused = $state(document.hasFocus());
  let metrics = $state<Metrics | null>(null);
  let snapshot = $state<Snapshot>({ caches: [], cacheScannedAt: 0, directory: null, dirty: false });
  let scanBusy = $state(false);
  let cleanBusy = $state(false);
  let progress = $state<Progress | null>(null);
  let filter = $state('all');
  let search = $state('');
  let sort = $state('cpu');
  let visibleProcesses = $state(50);
  let scanRoot = $state('home');
  let notice = $state<{ text: string; error: boolean } | null>(null);
  let confirmation = $state<Confirmation | null>(null);
  let smartOpen = $state(false);
  let smartReady = $state(false);
  let smartEntries = $state<Entry[]>([]);
  let smartReport = $state<BatchReport | null>(null);
  let cleanupProgress = $state<CleanupProgress | null>(null);
  let stoppingCleanup = $state(false);
  let dialog: HTMLDialogElement;
  let polling: ReturnType<typeof setTimeout> | undefined;
  let pollGeneration = 0;
  let sampling = false;
  let destroyed = false;
  const navigation = [
    { id: 'overview' as Page, icon: LayoutDashboard },
    { id: 'cleanup' as Page, icon: HardDrive },
    { id: 'analysis' as Page, icon: Database },
    { id: 'monitor' as Page, icon: Activity },
  ];
  const footprint = $derived(snapshot.caches.reduce((sum, entry) => sum + entry.bytes, 0));
  const cacheEntries = $derived(
    snapshot.caches.filter((entry) => filter === 'all' || entry.group === filter),
  );
  const processes = $derived(
    [...(metrics?.processes ?? [])]
      .filter(
        (p) =>
          p.name.toLowerCase().includes(search.toLowerCase()) || String(p.pid).includes(search),
      )
      .sort((a, b) =>
        sort === 'memory' ? b.memory - a.memory : b.cpu - a.cpu || b.memory - a.memory,
      ),
  );
  const directoryTotal = $derived(
    snapshot.directory?.entries.reduce((sum, entry) => sum + entry.bytes, 0) ?? 0,
  );
  const otherDirectoryBytes = $derived(
    snapshot.directory?.entries
      .filter((e) => e.bytes > 0)
      .slice(12)
      .reduce((sum, e) => sum + e.bytes, 0) ?? 0,
  );
  const memoryPercent = $derived(
    metrics?.memoryTotal ? (metrics.memoryUsed / metrics.memoryTotal) * 100 : 0,
  );
  const diskPercent = $derived(
    metrics?.diskTotal ? (1 - metrics.diskAvailable / metrics.diskTotal) * 100 : 0,
  );
  const busy = $derived(scanBusy || cleanBusy);

  function bytes(value: number | undefined, signed = false): string {
    if (value === undefined) return '—';
    const absolute = Math.abs(value);
    const units = ['B', 'KB', 'MB', 'GB', 'TB'];
    const index = absolute > 0 ? Math.min(4, Math.floor(Math.log(absolute) / Math.log(1000))) : 0;
    return `${signed && value > 0 ? '+' : value < 0 ? '−' : ''}${new Intl.NumberFormat(locale, { maximumFractionDigits: index < 2 ? 0 : 1 }).format(absolute / 1000 ** index)} ${units[index]}`;
  }
  function time(value: number): string {
    return new Intl.DateTimeFormat(locale === 'zh' ? 'zh-CN' : 'en', {
      month: 'short',
      day: 'numeric',
      hour: '2-digit',
      minute: '2-digit',
    }).format(value * 1000);
  }
  function showError(error: unknown) {
    const message = String(error);
    const code = message.split(':')[0];
    notice = { text: code in copy ? t(code) : `${t('error')}: ${message}`, error: true };
  }
  async function command<T>(name: string, args: Record<string, unknown> = {}): Promise<T> {
    if (!native) throw new Error('nativeOnly');
    return invoke<T>(name, args);
  }
  async function refreshMetrics() {
    if (sampling || destroyed || !native) return;
    sampling = true;
    try {
      const sample = await command<Metrics>('get_metrics');
      if (sample.sampledAt !== metrics?.sampledAt)
        cpuHistory = [...cpuHistory.slice(-29), sample.cpu];
      metrics = sample;
    } catch (error) {
      showError(error);
    } finally {
      sampling = false;
    }
  }
  function schedule() {
    clearTimeout(polling);
    const generation = ++pollGeneration;
    if (paused || !focused || document.hidden || !native || destroyed) return;
    polling = setTimeout(
      async () => {
        await refreshMetrics();
        if (generation === pollGeneration) schedule();
      },
      metrics?.lowPower ? Math.max(10000, interval) : interval,
    );
  }
  async function scan(mode: 'caches' | 'directory' | 'changed', root = scanRoot) {
    if (busy) return false;
    scanBusy = true;
    progress = null;
    notice = null;
    try {
      snapshot = await command<Snapshot>('start_scan', {
        mode,
        root: mode === 'directory' ? root : null,
        economy,
      });
      return true;
    } catch (error) {
      showError(error);
      return false;
    } finally {
      scanBusy = false;
      progress = null;
      await refreshMetrics();
    }
  }
  async function openSmartCleanup() {
    if (busy) return;
    smartReady = false;
    smartEntries = [];
    smartReport = null;
    smartOpen = true;
    if (await scan('caches')) {
      smartEntries = snapshot.caches;
      smartReady = true;
    }
  }
  function closeSmartCleanup() {
    if (cleanBusy) return;
    if (scanBusy) void command('cancel_scan').catch(showError);
    smartOpen = false;
  }
  async function cleanSelected(ids: string[]) {
    if (busy || !ids.length) return;
    cleanBusy = true;
    stoppingCleanup = false;
    cleanupProgress = null;
    notice = null;
    try {
      smartReport = await command<BatchReport>('clean_caches', { ids });
      snapshot = await command<Snapshot>('get_snapshot');
    } catch (error) {
      showError(error);
    } finally {
      cleanBusy = false;
      cleanupProgress = null;
      await refreshMetrics();
    }
  }
  async function stopCleanup() {
    stoppingCleanup = true;
    try {
      await command('cancel_scan');
    } catch (error) {
      stoppingCleanup = false;
      showError(error);
    }
  }
  async function pickDirectory() {
    try {
      const selected = await command<string | null>('choose_directory');
      if (selected) {
        scanRoot = selected;
        await scan('directory', selected);
      }
    } catch (error) {
      showError(error);
    }
  }
  async function reveal(path: string) {
    try {
      await command('reveal_path', { path });
    } catch (error) {
      showError(error);
    }
  }
  async function permissions() {
    try {
      await command('open_permissions');
    } catch (error) {
      showError(error);
    }
  }
  async function confirmAction() {
    if (!confirmation || busy) return;
    const selected = confirmation;
    cleanBusy = true;
    try {
      if (selected.kind === 'cache') {
        const result = await command<{ availableChange: number }>('clean_cache', {
          id: selected.entry.id,
        });
        snapshot = await command<Snapshot>('get_snapshot');
        notice = {
          text: t('cleaned', { size: bytes(result.availableChange, true) }),
          error: false,
        };
      } else {
        await command('manage_process', {
          pid: selected.process.pid,
          startTime: selected.process.startTime,
          action: selected.action,
        });
        notice = { text: t('processDone'), error: false };
      }
    } catch (error) {
      showError(error);
    } finally {
      cleanBusy = false;
      confirmation = null;
      dialog?.close();
      await refreshMetrics();
    }
  }
  function cacheStatus(entry: Entry) {
    return entry.errors
      ? 'incomplete'
      : entry.blocked.length
        ? 'inUse'
        : entry.bytes < 1024
          ? 'empty'
          : 'regenerable';
  }
  function canClean(entry: Entry) {
    return !busy && !entry.errors && !entry.blocked.length && entry.bytes >= 1024;
  }

  $effect(() => {
    localStorage.setItem(
      'macspace.preferences',
      JSON.stringify({ language, interval, economy, autoRefresh }),
    );
    document.documentElement.lang = locale === 'zh' ? 'zh-CN' : 'en';
  });
  $effect(() => {
    if (confirmation && dialog && !dialog.open) dialog.showModal();
  });
  $effect(() => {
    if (native) void command('set_ui_language', { language: locale }).catch(showError);
  });
  $effect(() => {
    interval;
    paused;
    focused;
    schedule();
  });
  $effect(() => {
    if (
      !native ||
      !autoRefresh ||
      smartOpen ||
      page !== 'cleanup' ||
      !focused ||
      busy ||
      !snapshot.cacheScannedAt ||
      !snapshot.dirty
    )
      return;
    const timer = setTimeout(
      () => {
        lastIncremental = Date.now();
        void scan('changed');
      },
      Math.max(8000, 30000 - (Date.now() - lastIncremental)),
    );
    return () => clearTimeout(timer);
  });

  onMount(() => {
    destroyed = false;
    const unlisten: (() => void)[] = [];
    const onFocus = async () => {
      focused = true;
      if (native && language === 'system') {
        try {
          systemLanguage = await command<'zh' | 'en'>('get_system_language');
        } catch {
          /* Keep browser locale until native locale is available. */
        }
      }
      if (!paused) await refreshMetrics();
      schedule();
    };
    const onBlur = () => {
      focused = false;
      clearTimeout(polling);
      pollGeneration++;
    };
    const onVisibility = () => {
      if (document.hidden) onBlur();
      else if (document.hasFocus()) void onFocus();
    };
    window.addEventListener('focus', onFocus);
    window.addEventListener('blur', onBlur);
    document.addEventListener('visibilitychange', onVisibility);
    if (native) {
      void (async () => {
        try {
          systemLanguage = await command<'zh' | 'en'>('get_system_language');
          snapshot = await command<Snapshot>('get_snapshot');
          await refreshMetrics();
          for (const [name, handler] of [
            [
              'cleanup-progress',
              (event: { payload: CleanupProgress }) => {
                cleanupProgress = event.payload;
              },
            ],
            [
              'scan-progress',
              (event: { payload: Progress }) => {
                progress = event.payload;
              },
            ],
            [
              'cache-dirty',
              () => {
                snapshot.dirty = true;
              },
            ],
          ] as const) {
            const stop = await listen(name, handler as (event: { payload: unknown }) => void);
            if (destroyed) stop();
            else unlisten.push(stop);
          }
          schedule();
        } catch (error) {
          showError(error);
        }
      })();
    }
    return () => {
      destroyed = true;
      clearTimeout(polling);
      pollGeneration++;
      unlisten.forEach((stop) => stop());
      window.removeEventListener('focus', onFocus);
      window.removeEventListener('blur', onBlur);
      document.removeEventListener('visibilitychange', onVisibility);
    };
  });
</script>

<svelte:head><title>{locale === 'zh' ? '清衡 · MacSpace' : 'MacSpace'}</title></svelte:head>

<div class="app-shell">
  <aside class="sidebar">
    <div class="brand">
      <div class="brand-icon"><HardDrive size={23} strokeWidth={1.8} /></div>
      <div><strong>{locale === 'zh' ? '清衡' : 'MacSpace'}</strong><span>MACSPACE</span></div>
    </div>
    <p class="tagline">{t('tagline')}</p>
    <nav aria-label={locale === 'zh' ? '主导航' : 'Main navigation'}>
      {#each navigation as item}
        <button
          class:active={page === item.id}
          aria-current={page === item.id ? 'page' : undefined}
          onclick={() => {
            page = item.id;
            notice = null;
          }}
          ><item.icon size={18} /><span>{t(item.id)}</span
          >{#if item.id === 'cleanup' && snapshot.dirty && snapshot.cacheScannedAt}<i
              class="nav-dot"
            ></i>{/if}</button
        >
      {/each}
    </nav>
    <div class="sidebar-bottom">
      <div class="quiet-note">
        <Leaf size={16} /><span>{t(metrics?.lowPower ? 'lowPower' : 'normalPower')}</span>
      </div>
      <button class:active={page === 'settings'} onclick={() => (page = 'settings')}
        ><Settings2 size={18} /><span>{t('settings')}</span><small>0.2.0</small></button
      >
      <div class="local-note"><ShieldCheck size={13} /><span>{t('localOnly')}</span></div>
    </div>
  </aside>

  <main>
    <header class="topbar">
      <div class="breadcrumb">MACSPACE <ChevronRight size={13} /><span>{t(page)}</span></div>
      <div class="header-actions">
        <span class="sampling-state"
          ><i class:muted={!focused || paused}></i>{t(
            paused ? 'paused' : focused ? 'live' : 'focusPaused',
          )}</span
        ><label class="language-picker"
          ><Languages size={16} /><select aria-label={t('language')} bind:value={language}
            ><option value="system">{t('system')}</option><option value="zh">中文</option><option
              value="en">English</option
            ></select
          ></label
        >
      </div>
    </header>
    <div class="page-content">
      <div class="page-heading">
        <div>
          <div class="eyebrow">
            {t(`${page}Eyebrow`)}
          </div>
          <h1>{t(`${page}Title`)}</h1>
          <p>{t(`${page}Sub`)}</p>
        </div>
        {#if page === 'overview' || page === 'cleanup'}<div class="page-heading-actions">
            <button class="primary" disabled={busy || !native} onclick={openSmartCleanup}
              ><Sparkles size={16} />{t('smartClean')}</button
            ><button
              class="secondary"
              disabled={busy || !native}
              onclick={() => {
                page = 'cleanup';
                void scan('caches');
              }}
              >{#if scanBusy}<LoaderCircle size={16} class="spin" />{:else}<Search
                  size={16}
                />{/if}{t(scanBusy ? 'scanning' : 'scanCaches')}</button
            >
          </div>{:else if page === 'monitor'}<button
            class="secondary"
            onclick={() => {
              paused = !paused;
              if (!paused) void refreshMetrics();
            }}
            >{#if paused}<Play size={15} />{:else}<Pause size={15} />{/if}{t(
              paused ? 'resume' : 'pause',
            )}</button
          >{/if}
      </div>

      {#if !native}<div class="info-banner"><CircleHelp size={17} />{t('nativeOnly')}</div>{/if}
      {#if notice}<div class="notice" class:error={notice.error} role="status">
          <span
            >{#if !notice.error}<Check size={17} />{/if}{notice.text}</span
          ><button class="icon-button" aria-label={t('dismiss')} onclick={() => (notice = null)}
            ><X size={16} /></button
          >
        </div>{/if}
      {#if scanBusy}<div class="scan-progress" role="status">
          <div class="scan-line">
            <span><LoaderCircle size={15} class="spin" />{t('scanning')}</span><button
              class="text-button"
              onclick={() => command('cancel_scan').catch(showError)}>{t('cancel')}</button
            >
          </div>
          <div class="progress-track"><div></div></div>
          <div class="scan-path" title={progress?.path}>
            {progress
              ? `${t('files', { count: progress.files.toLocaleString(locale) })} · ${bytes(progress.bytes)} · ${progress.path}`
              : t('working')}
          </div>
        </div>{/if}

      {#if page === 'overview' || page === 'monitor'}
        <section
          class="metrics-grid"
          aria-label={locale === 'zh' ? '资源概况' : 'Resource overview'}
        >
          <article class="metric">
            <div class="metric-label"><Cpu size={17} />{t('cpu')}</div>
            <div class="metric-value">
              {metrics ? metrics.cpu.toFixed(1) : '—'}<span>%</span>{#if cpuHistory.length > 1}<svg
                  class="mini-wave"
                  viewBox="0 0 90 30"
                  aria-hidden="true"
                  ><path
                    d={cpuHistory
                      .map(
                        (value, index) =>
                          `${index === 0 ? 'M' : 'L'}${(index / (cpuHistory.length - 1)) * 90} ${28 - Math.min(100, value) * 0.26}`,
                      )
                      .join(' ')}
                  /></svg
                >{/if}
            </div>
            <small>{t('physicalCores', { count: metrics?.cores ?? '—' })}</small>
          </article>
          <article class="metric">
            <div class="metric-label"><MemoryStick size={17} />{t('memory')}</div>
            <div class="metric-value">
              {bytes(metrics?.memoryUsed)}<span>/ {bytes(metrics?.memoryTotal)}</span>
            </div>
            <div class="thin-track"><i style={`width:${memoryPercent}%`}></i></div>
            <small>{t('swap', { size: bytes(metrics?.swapUsed) })}</small>
          </article>
          <article class="metric">
            <div class="metric-label"><Thermometer size={17} />{t('temperature')}</div>
            <div class="metric-value">
              {metrics?.sensors.length ? metrics.sensors[0].celsius.toFixed(1) : '—'}<span>°C</span>
            </div>
            <small
              >{metrics?.sensors.length ? t('highestSensor') : t('noSensor')}<span
                class="thermal-dot"
                class:warm={(metrics?.thermalState ?? 0) > 1}
              ></span>{metrics ? t(`thermal${metrics.thermalState}`) : ''}</small
            >
          </article>
          <article class="metric">
            <div class="metric-label"><HardDrive size={17} />{t('freeSpace')}</div>
            <div class="metric-value">{bytes(metrics?.diskAvailable)}</div>
            <small
              >{metrics
                ? `${(100 - diskPercent).toFixed(1)}% · ${bytes(metrics.diskTotal)} ${t('total')}`
                : '—'}</small
            >
          </article>
        </section>
      {/if}

      {#if page === 'overview'}
        <section class="storage-panel panel">
          <div class="section-heading">
            <div>
              <span class="eyebrow">STORAGE</span>
              <h2>{t('storage')}</h2>
            </div>
            <span class="chip">Macintosh HD</span>
          </div>
          <div class="storage-overview">
            <div class="storage-amount">
              {metrics ? bytes(metrics.diskTotal - metrics.diskAvailable) : '—'}<span
                >{t('used')} / {bytes(metrics?.diskTotal)}</span
              >
            </div>
            <span class="available-pill">{bytes(metrics?.diskAvailable)} {t('available')}</span>
          </div>
          <div class="storage-bar"><div style={`width:${diskPercent}%`}></div></div>
          <div class="storage-legend">
            <span><i></i>{t('used')}</span><span><i></i>{t('available')}</span><small
              >{t('diskCaption')}</small
            >
          </div>
        </section>
        <div class="overview-bottom">
          <section class="cache-feature">
            <div class="feature-icon"><Trash2 size={21} /></div>
            <span class="eyebrow">ROOM TO BREATHE</span>
            <h2>{t('reclaimable')}</h2>
            <div class="cache-big-number">{snapshot.cacheScannedAt ? bytes(footprint) : '—'}</div>
            <p>{snapshot.cacheScannedAt ? t('estimated') : t('scanPromptSub')}</p>
            <button class="feature-link" onclick={() => (page = 'cleanup')}
              >{t('cleanup')}<ArrowRight size={18} /></button
            >
          </section>
          <section class="panel active-processes">
            <div class="section-heading">
              <h2>{t('topProcesses')}</h2>
              <button class="text-button" onclick={() => (page = 'monitor')}
                >{t('viewAll')}<ArrowRight size={14} /></button
              >
            </div>
            {#each (metrics?.processes ?? []).slice(0, 5) as process}<div class="compact-process">
                <span class="process-avatar">{process.name.slice(0, 1).toUpperCase()}</span>
                <div><strong>{process.name}</strong><small>PID {process.pid}</small></div>
                <span class="mono">{process.cpu.toFixed(1)}%</span><span class="muted-text mono"
                  >{bytes(process.memory)}</span
                >
              </div>{/each}
          </section>
        </div>
      {:else if page === 'cleanup'}
        <div class="cleanup-summary">
          <div>
            <span>{t('reclaimable')}</span><strong
              >{snapshot.cacheScannedAt ? bytes(footprint) : '—'}</strong
            ><small>{t('estimated')}</small>
          </div>
          <div class="scan-meta">
            <span class="chip" class:changed={snapshot.dirty && snapshot.cacheScannedAt > 0}
              >{t(
                !snapshot.cacheScannedAt ? 'neverScanned' : snapshot.dirty ? 'changes' : 'upToDate',
              )}</span
            >{#if snapshot.cacheScannedAt}<small
                >{t('lastScan')} {time(snapshot.cacheScannedAt)}</small
              >{/if}
          </div>
        </div>
        <div class="filter-row" role="group" aria-label={t('categoryAll')}>
          {#each ['all', 'development', 'packages', 'updates', 'browser'] as category}<button
              class:selected={filter === category}
              onclick={() => (filter = category)}
              >{t(category === 'all' ? 'categoryAll' : category)}</button
            >{/each}
        </div>
        {#if !snapshot.cacheScannedAt}<section class="empty-state panel">
            <div class="empty-icon"><HardDrive size={32} /></div>
            <h2>{t('scanPrompt')}</h2>
            <p>{t('scanPromptSub')}</p>
            <button class="primary" disabled={busy || !native} onclick={() => scan('caches')}
              ><Search size={16} />{t('scanCaches')}</button
            >
          </section>{:else}<div class="panel table-panel">
            <table>
              <thead
                ><tr
                  ><th>{t('cacheName')}</th><th>{t('footprint')}</th><th>{t('status')}</th><th
                    class="align-right">{t('actions')}</th
                  ></tr
                ></thead
              ><tbody
                >{#each cacheEntries as entry}<tr
                    ><td
                      ><div class="resource-cell">
                        <span class="resource-icon"><Folder size={19} /></span>
                        <div>
                          <strong>{t(entry.id)}</strong><small title={entry.path}
                            >{entry.path.replace(/^\/Users\/[^/]+/, '~')}</small
                          >
                        </div>
                      </div></td
                    ><td class="mono">{bytes(entry.bytes)}</td><td
                      ><span
                        class="status-badge"
                        class:warning={entry.blocked.length || entry.errors}
                        title={entry.blocked.join(', ')}>{t(cacheStatus(entry))}</span
                      ></td
                    ><td
                      ><div class="row-actions">
                        <button
                          class="icon-button"
                          aria-label={t('reveal')}
                          onclick={() => reveal(entry.path)}><ArrowDownRight size={16} /></button
                        ><button
                          class="small-button"
                          disabled={!canClean(entry)}
                          onclick={() => (confirmation = { kind: 'cache', entry })}
                          >{t('clean')}</button
                        >
                      </div></td
                    ></tr
                  >{/each}</tbody
              >
            </table>
            {#if !cacheEntries.length}<p class="table-empty">{t('noMatches')}</p>{/if}
          </div>{/if}
        <div class="footnote">
          <ShieldCheck size={16} />
          <p>{t('scanHint')}<br />{t('cleanupHint')}</p>
        </div>
      {:else if page === 'analysis'}
        <div class="location-toolbar">
          <label
            ><Folder size={17} /><select aria-label={t('directory')} bind:value={scanRoot}
              ><option value="home">{t('home')}</option><option value="disk"
                >{t('entireDisk')}</option
              >{#if scanRoot !== 'home' && scanRoot !== 'disk'}<option value={scanRoot}
                  >{scanRoot}</option
                >{/if}</select
            ></label
          ><button class="secondary" disabled={busy || !native} onclick={pickDirectory}
            >{t('chooseFolder')}</button
          ><button class="primary" disabled={busy || !native} onclick={() => scan('directory')}
            ><Search size={16} />{t('scanDirectory')}</button
          >
        </div>
        {#if snapshot.directory}<section class="panel directory-panel">
            <div class="section-heading">
              <div>
                <span class="eyebrow">{t('scanned')}</span>
                <h2 class="directory-total">{bytes(directoryTotal)}</h2>
                <p class="path-label">{snapshot.directory.root}</p>
              </div>
              <div class="scan-meta">
                {#if snapshot.directory.cancelled || snapshot.directory.errors}<span
                    class="chip changed">{t('partial')}</span
                  >{/if}<small>{time(snapshot.directory.scannedAt)}</small><small
                  >{t('files', { count: snapshot.directory.files.toLocaleString(locale) })}</small
                >
              </div>
            </div>
            <div class="distribution-bar" aria-label={t('footprint')}>
              {#each snapshot.directory.entries
                .filter((e) => e.bytes > 0)
                .slice(0, 12) as entry, index}<button
                  class={`segment segment-${index % 5}`}
                  style={`flex-grow:${entry.bytes}`}
                  title={`${entry.name}: ${bytes(entry.bytes)}`}
                  aria-label={`${entry.name}: ${bytes(entry.bytes)}`}
                  disabled={busy || !entry.isDirectory}
                  onclick={() => {
                    scanRoot = entry.path;
                    void scan('directory', entry.path);
                  }}
                ></button>{/each}
              {#if otherDirectoryBytes > 0}<div
                  class="segment segment-other"
                  style={`flex-grow:${otherDirectoryBytes}`}
                  title={`${t('other')}: ${bytes(otherDirectoryBytes)}`}
                ></div>{/if}
            </div>
            <div class="directory-list">
              {#each snapshot.directory.entries as entry, index}<div class="directory-row">
                  <span class={`directory-index color-${index % 5}`}
                    >{String(index + 1).padStart(2, '0')}</span
                  ><button
                    class="directory-name"
                    disabled={busy || !entry.isDirectory}
                    onclick={() => {
                      scanRoot = entry.path;
                      void scan('directory', entry.path);
                    }}
                    >{#if entry.isDirectory}<Folder size={17} />{:else}<File size={17} />{/if}<span
                      >{entry.name}</span
                    >{#if entry.errors}<small>{t('incomplete')}</small>{/if}</button
                  >
                  <div class="directory-meter">
                    <i style={`width:${directoryTotal ? (entry.bytes / directoryTotal) * 100 : 0}%`}
                    ></i>
                  </div>
                  <span class="mono">{bytes(entry.bytes)}</span><button
                    class="icon-button"
                    aria-label={t('reveal')}
                    onclick={() => reveal(entry.path)}><ArrowDownRight size={16} /></button
                  >
                </div>{/each}
            </div>
          </section>
          {#if snapshot.directory.errors}<div class="info-banner">
              <CircleHelp size={17} />
              <div>
                <strong>{t('scanErrors', { count: snapshot.directory.errors })}</strong>
                <p>{t('permissionHint')}</p>
                <button class="text-button" onclick={permissions}
                  >{t('permission')}<ArrowRight size={14} /></button
                >
              </div>
            </div>{/if}{#if snapshot.directory.skippedMounts}<p class="small-note">
              {t('mountsSkipped', { count: snapshot.directory.skippedMounts })}
            </p>{/if}{:else}<section class="empty-state panel">
            <div class="empty-icon"><Database size={32} /></div>
            <h2>{t('analyseEmpty')}</h2>
            <p>{t('analyseEmptySub')}</p>
          </section>{/if}
        <div class="footnote">
          <ShieldCheck size={16} />
          <p>{t('scanDirectoryHint')}<br />{t('directoryNote')}<br />{t('noDeleteExplorer')}</p>
        </div>
      {:else if page === 'monitor'}
        <div class="monitor-grid">
          <section class="panel thermal-panel">
            <div class="section-heading">
              <h2>{t('sensors')}</h2>
              <span class="chip" class:changed={(metrics?.thermalState ?? 0) > 1}
                >{t('thermal')} · {metrics ? t(`thermal${metrics.thermalState}`) : '—'}</span
              >
            </div>
            {#if metrics?.sensors.length}<div class="sensor-list">
                {#each metrics.sensors.slice(0, 12) as sensor}<div>
                    <span>{sensor.name}</span><strong class="mono"
                      >{sensor.celsius.toFixed(1)} <small>°C</small></strong
                    >
                  </div>{/each}
              </div>
              <p class="small-note">{t('sensorNote')}</p>{:else}<p class="sensor-unavailable">
                {t('sensorMissing')}
              </p>{/if}
          </section>
          <section class="panel load-panel">
            <h2>{t('loadAverage')}</h2>
            <div class="load-values">
              {#each [0, 1, 2] as index}<div>
                  <strong>{metrics?.loadAverage[index].toFixed(2) ?? '—'}</strong><span
                    >{[1, 5, 15][index]} min</span
                  >
                </div>{/each}
            </div>
            <p class="small-note">{t('loadNote')}</p>
          </section>
        </div>
        <div class="process-toolbar">
          <label class="search-field"
            ><Search size={17} /><input
              aria-label={t('searchProcess')}
              placeholder={t('searchProcess')}
              bind:value={search}
              oninput={() => (visibleProcesses = 50)}
            /></label
          ><select aria-label={t('actions')} bind:value={sort}
            ><option value="cpu">{t('sortCpu')}</option><option value="memory"
              >{t('sortMemory')}</option
            ></select
          ><span class="muted-text"
            >{metrics?.processCount ?? 0} {locale === 'zh' ? '个进程' : 'processes'}</span
          >
        </div>
        <div class="panel table-panel process-table">
          <table>
            <thead
              ><tr
                ><th>{t('process')}</th><th>{t('cpuPerCore')}</th><th>{t('rss')}</th><th
                  class="align-right">{t('actions')}</th
                ></tr
              ></thead
            ><tbody
              >{#each processes.slice(0, visibleProcesses) as process (process.pid)}<tr
                  ><td
                    ><div class="process-name">
                      <strong>{process.name}</strong><small
                        >PID {process.pid}{process.nice >= 10
                          ? ` · nice ${process.nice}`
                          : ''}</small
                      >
                    </div></td
                  ><td class="mono">{process.cpu.toFixed(1)}%</td><td class="mono"
                    >{bytes(process.memory)}</td
                  ><td
                    ><div class="row-actions">
                      <button
                        class="small-button"
                        disabled={busy || !process.manageable || process.nice >= 10}
                        title={!process.manageable ? t('protected') : t('lowerBody')}
                        onclick={() =>
                          (confirmation = { kind: 'process', process, action: 'lower' })}
                        ><ArrowDown size={13} />{t('lower')}</button
                      ><button
                        class="icon-button danger-button"
                        disabled={busy || !process.manageable}
                        aria-label={`${t('terminate')} ${process.name}`}
                        onclick={() =>
                          (confirmation = { kind: 'process', process, action: 'terminate' })}
                        ><Square size={13} /></button
                      >
                    </div></td
                  ></tr
                >{/each}</tbody
            >
          </table>
          {#if !processes.length}<p class="table-empty">
              {t('noMatches')}
            </p>{/if}{#if processes.length > visibleProcesses}<button
              class="show-more"
              onclick={() => (visibleProcesses += 50)}>{t('showMore')}</button
            >{/if}
        </div>
        <p class="small-note">{t('processNote')} {t('focusPaused')}</p>
      {:else if page === 'settings'}
        <section class="panel settings-panel">
          <div class="setting-row">
            <div>
              <h2><Languages size={19} />{t('language')}</h2>
              <p>{t('languageHint')}</p>
            </div>
            <select bind:value={language} aria-label={t('language')}
              ><option value="system">{t('system')}</option><option value="zh">中文</option><option
                value="en">English</option
              ></select
            >
          </div>
          <div class="setting-row">
            <div>
              <h2><Activity size={19} />{t('sampling')}</h2>
              <p>{t('samplingHint')}</p>
            </div>
            <select bind:value={interval} aria-label={t('sampling')}
              >{#each [3000, 5000, 10000] as value}<option {value}
                  >{t('seconds', { count: value / 1000 })}</option
                >{/each}</select
            >
          </div>
          <div class="setting-row">
            <div>
              <h2><Leaf size={19} />{t('economy')}</h2>
              <p>{t('economyHint')}</p>
            </div>
            <label class="switch"
              ><input type="checkbox" bind:checked={economy} aria-label={t('economy')} /><span
              ></span></label
            >
          </div>
          <div class="setting-row">
            <div>
              <h2><RefreshCw size={19} />{t('watching')}</h2>
              <p>{t('watchingHint')}</p>
            </div>
            <span class="status-badge">{snapshot.watching ? 'FSEvents' : t('neverScanned')}</span>
          </div>
          <div class="setting-row">
            <div>
              <h2><RefreshCw size={19} />{t('autoRefresh')}</h2>
              <p>{t('autoRefreshHint')}</p>
            </div>
            <label class="switch"
              ><input
                type="checkbox"
                bind:checked={autoRefresh}
                aria-label={t('autoRefresh')}
              /><span></span></label
            >
          </div>
          <div class="setting-row">
            <div>
              <h2><ShieldCheck size={19} />{t('permission')}</h2>
              <p>{t('permissionHint')}</p>
            </div>
            <button class="secondary" disabled={!native} onclick={permissions}
              >{t('permission')}<ArrowRight size={14} /></button
            >
          </div>
        </section>
        <section class="privacy-note">
          <ShieldCheck size={24} />
          <div>
            <h2>{t('privacy')}</h2>
            <p>{t('privacyHint')}</p>
          </div>
        </section>
        <div class="about-line">
          <span>{t('version')} 0.2.0</span><span>{t('buildTech')}</span><span
            >{metrics?.chip ?? 'Apple Silicon'} · {metrics?.os ?? 'macOS'}</span
          >
        </div>
      {/if}
    </div>
    <footer>
      <span><ShieldCheck size={12} />{t('localOnly')}</span><span
        >{metrics?.sampledAt ? time(metrics.sampledAt) : t('ready')}</span
      >
    </footer>
  </main>
</div>

{#if smartOpen}
  <SmartCleanup
    entries={smartEntries}
    ready={smartReady}
    scanning={scanBusy}
    cleaning={cleanBusy}
    stopping={stoppingCleanup}
    progress={cleanupProgress}
    scanDetail={progress
      ? `${t('files', { count: progress.files.toLocaleString(locale) })} · ${bytes(progress.bytes)} · ${progress.path}`
      : ''}
    report={smartReport}
    error={notice?.error ? notice.text : null}
    {t}
    {bytes}
    onClean={cleanSelected}
    onClose={closeSmartCleanup}
    onStop={stopCleanup}
    onRetry={openSmartCleanup}
  />
{/if}

<dialog
  bind:this={dialog}
  oncancel={(event) => {
    if (cleanBusy) event.preventDefault();
    else confirmation = null;
  }}
  onclose={() => {
    confirmation = null;
  }}
>
  {#if confirmation}<div class="confirm-dialog">
      <div class="dialog-icon">
        {#if confirmation.kind === 'cache'}<Trash2 size={25} />{:else}<Activity size={25} />{/if}
      </div>
      <h2>
        {confirmation.kind === 'cache'
          ? t('cleanTitle', { name: t(confirmation.entry.id) })
          : t('processTitle', { name: confirmation.process.name, action: t(confirmation.action) })}
      </h2>
      <p>
        {confirmation.kind === 'cache'
          ? t('cleanBody', { size: bytes(confirmation.entry.bytes) })
          : t(confirmation.action === 'lower' ? 'lowerBody' : 'terminateBody')}
      </p>
      <code
        >{confirmation.kind === 'cache'
          ? confirmation.entry.path
          : `PID ${confirmation.process.pid}`}</code
      >
      <div class="dialog-actions">
        <button
          class="secondary"
          disabled={cleanBusy}
          onclick={() => {
            confirmation = null;
            dialog.close();
          }}>{t('cancel')}</button
        ><button class="primary" disabled={cleanBusy} onclick={confirmAction}
          >{#if cleanBusy}<LoaderCircle size={15} class="spin" />{/if}{t(
            cleanBusy
              ? 'working'
              : confirmation.kind === 'cache'
                ? 'confirmClean'
                : 'confirmProcess',
          )}</button
        >
      </div>
    </div>{/if}
</dialog>
