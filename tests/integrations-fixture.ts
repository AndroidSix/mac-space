import { mount } from 'svelte';
import '../src/style.css';
import type { ApiConfig, Metrics } from '../src/types';

const params = new URLSearchParams(location.search);
localStorage.setItem(
  'macspace.preferences',
  JSON.stringify({ language: params.get('language') === 'en' ? 'en' : 'zh', interval: 10000 }),
);
let paused = false;
let savedToken = false;
let callback = 0;
let api: ApiConfig = { enabled: false, port: 47653, token: 'fixture-token-not-a-real-credential' };
let metrics: Metrics = {
  cpu: 21,
  cores: 8,
  memoryUsed: 8e9,
  memoryTotal: 16e9,
  swapUsed: 0,
  diskTotal: 500e9,
  diskAvailable: 70e9,
  thermalState: 0,
  lowPower: false,
  sensors: [{ name: 'Fixture sensor', celsius: 48 }],
  processes: [],
  processCount: 0,
  loadAverage: [1, 1, 1],
  sampledAt: 1,
  chip: 'Fixture Apple Silicon',
  os: params.has('windows') ? 'Windows fixture' : 'macOS fixture',
  platform: params.has('windows') ? 'windows' : 'macos',
  power: {
    current: 'auto',
    modes: [
      { id: 'auto', name: 'powerAuto' },
      { id: 'low', name: 'powerLow' },
    ],
    requiresAdmin: !params.has('windows'),
  },
};

Object.assign(window, {
  __TAURI_INTERNALS__: {
    transformCallback: () => ++callback,
    invoke: async (command: string, args: Record<string, unknown> = {}) => {
      switch (command) {
        case 'get_system_language':
          return 'zh';
        case 'set_ui_language':
        case 'plugin:event|unlisten':
        case 'open_releases':
          return;
        case 'plugin:event|listen':
          return callback;
        case 'get_snapshot':
          return { caches: [], cacheScannedAt: 0, directory: null, dirty: false };
        case 'get_power_status':
          return metrics.power;
        case 'set_power_mode':
          if (params.has('powerDenied')) throw 'power_denied';
          metrics = { ...metrics, power: { ...metrics.power, current: String(args.id) } };
          return metrics.power;
        case 'get_metrics':
          if (!paused) metrics = { ...metrics, sampledAt: Math.floor(Date.now() / 1000) };
          return metrics;
        case 'set_sampling_paused':
          paused = args.paused === true;
          return;
        case 'get_local_api':
          return { ...api };
        case 'set_local_api':
          if (params.has('portBusy') && args.enabled) throw 'api_port_busy';
          api = {
            ...api,
            enabled: args.enabled === true,
            token: args.rotate ? 'rotated-fixture-token' : api.token,
          };
          return { ...api };
        case 'has_update_token':
          return savedToken;
        case 'save_update_token':
          savedToken = !!args.value;
          return;
        case 'check_update':
          if (params.has('authError')) throw 'update_auth';
          return {
            current: '0.3.0',
            latest: '0.4.0',
            available: true,
            url: 'https://github.com/AndroidSix/mac-space/releases',
            checkedAt: Math.floor(Date.now() / 1000),
          };
        default:
          throw 'invalid_action';
      }
    },
  },
  __TAURI_EVENT_PLUGIN_INTERNALS__: { unregisterListener: () => {} },
});

const { default: App } = await import('../src/App.svelte');
mount(App, { target: document.getElementById('fixture')! });
