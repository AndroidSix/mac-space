<script lang="ts">
  import { onMount } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { ArrowUpRight, KeyRound, LoaderCircle, Radio, RefreshCw } from '@lucide/svelte';
  import type { MessageKey } from './i18n';
  import type { ApiConfig, UpdateInfo } from './types';

  let {
    t,
    native,
  }: {
    t: (key: MessageKey | string, values?: Record<string, string | number>) => string;
    native: boolean;
  } = $props();
  let api = $state<ApiConfig | null>(null);
  let busy = $state(false);
  let checking = $state(false);
  let saving = $state(false);
  let credential = $state('');
  let savedToken = $state(false);
  let reveal = $state(false);
  let result = $state<UpdateInfo | null>(null);
  let apiError = $state('');
  let updateError = $state('');
  let credentialError = $state('');
  const errorKey = (error: unknown) => String(error).split(':')[0];

  onMount(() => {
    if (!native) return;
    void invoke<ApiConfig>('get_local_api')
      .then((value) => (api = value))
      .catch((error) => (apiError = errorKey(error)));
    void invoke<boolean>('has_update_token')
      .then((value) => (savedToken = value))
      .catch((error) => (credentialError = errorKey(error)));
  });

  async function configure(enabled: boolean, rotate = false) {
    busy = true;
    apiError = '';
    try {
      api = await invoke<ApiConfig>('set_local_api', { enabled, rotate });
    } catch (error) {
      apiError = errorKey(error);
    } finally {
      busy = false;
    }
  }
  async function saveToken(clear = false) {
    saving = true;
    credentialError = '';
    try {
      await invoke('save_update_token', { value: clear ? '' : credential.trim() });
      savedToken = !clear;
      credential = '';
      result = null;
    } catch (error) {
      credentialError = errorKey(error);
    } finally {
      saving = false;
    }
  }
  async function checkUpdate() {
    checking = true;
    updateError = '';
    result = null;
    try {
      result = await invoke<UpdateInfo>('check_update');
    } catch (error) {
      updateError = errorKey(error);
    } finally {
      checking = false;
    }
  }
  async function openReleases() {
    try {
      await invoke('open_releases');
    } catch (error) {
      updateError = errorKey(error);
    }
  }
</script>

<section class="panel settings-panel integration-panel" aria-label={t('appUpdates')}>
  <div class="setting-row">
    <div>
      <h2><RefreshCw size={19} />{t('appUpdates')}</h2>
      <p>{t('updatesHint')}</p>
    </div>
    <button class="secondary" disabled={!native || checking} onclick={checkUpdate}>
      {#if checking}<LoaderCircle size={15} class="spin" />{:else}<RefreshCw size={15} />{/if}{t(
        checking ? 'checkingUpdate' : 'checkUpdate',
      )}
    </button>
  </div>
  <div class="integration-result" role="status">
    {#if result}
      <strong
        >{t(result.available ? 'updateAvailable' : 'updateCurrent', {
          version: result.available ? result.latest : result.current,
        })}</strong
      >
      <p>{t('updateChecked', { time: new Date(result.checkedAt * 1000).toLocaleString() })}</p>
    {/if}
    {#if updateError}<p class="inline-error">{t(updateError)}</p>{/if}
    <button class="small-button" disabled={!native} onclick={openReleases}
      >{t(result?.available ? 'downloadUpdate' : 'openReleases')}<ArrowUpRight size={14} /></button
    >
  </div>
  <div class="setting-row credential-row">
    <div>
      <h2><KeyRound size={19} />{t('githubToken')}</h2>
      <p>{t('githubTokenHint')}</p>
    </div>
    <span class="status-badge">{t(savedToken ? 'tokenSaved' : 'tokenMissing')}</span>
  </div>
  <div class="integration-fields">
    <label for="github-token">{t('githubToken')}</label>
    <div class="integration-actions">
      <input
        id="github-token"
        type="password"
        autocomplete="off"
        spellcheck="false"
        bind:value={credential}
        placeholder="github_pat_…"
        disabled={!native || saving}
      />
      <button
        class="secondary"
        disabled={!native || saving || !credential.trim()}
        onclick={() => saveToken()}>{t(saving ? 'working' : 'saveToken')}</button
      >
      <button
        class="small-button"
        disabled={!native || saving || !savedToken}
        onclick={() => saveToken(true)}>{t('removeToken')}</button
      >
    </div>
    {#if credentialError}<p class="inline-error" role="status">{t(credentialError)}</p>{/if}
  </div>
</section>

<section class="panel settings-panel integration-panel" aria-label={t('localApi')}>
  <div class="setting-row">
    <div>
      <h2><Radio size={19} />{t('localApi')}</h2>
      <p>{t('localApiHint')}</p>
    </div>
    <label class="switch"
      ><input
        type="checkbox"
        checked={api?.enabled ?? false}
        disabled={!native || !api || busy}
        onchange={(event) => {
          const enabled = event.currentTarget.checked;
          event.currentTarget.checked = api?.enabled ?? false;
          void configure(enabled);
        }}
        aria-label={t('localApi')}
      /><span></span></label
    >
  </div>
  {#if api}
    <div class="integration-fields">
      <label for="api-address">{t('apiAddress')}</label>
      <input id="api-address" readonly value={`http://127.0.0.1:${api.port}/v1`} />
      <label for="api-token">{t('apiToken')}</label>
      <div class="integration-actions">
        <input
          id="api-token"
          type={reveal ? 'text' : 'password'}
          readonly
          value={api.token}
          spellcheck="false"
        />
        <button class="small-button" aria-pressed={reveal} onclick={() => (reveal = !reveal)}
          >{t(reveal ? 'hideToken' : 'showToken')}</button
        >
        <button
          class="small-button"
          disabled={busy || api.enabled}
          title={t('rotateTokenHint')}
          onclick={() => configure(false, true)}>{t('rotateToken')}</button
        >
      </div>
      <p>{t('apiUsage')}</p>
      <code>Authorization: Bearer &lt;token&gt;</code>
      <code>GET /v1/status · /v1/metrics · /v1/snapshot · /v1/events</code>
      <p>{t('apiEventsHint')}</p>
    </div>
  {/if}
  {#if apiError}<p class="inline-error" role="status">{t(apiError)}</p>{/if}
</section>
