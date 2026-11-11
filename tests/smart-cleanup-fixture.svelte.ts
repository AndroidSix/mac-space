import { mount, unmount } from 'svelte';
import SmartCleanup from '../src/SmartCleanup.svelte';
import { messages, type MessageKey } from '../src/i18n';
import type { BatchReport, Entry } from '../src/types';
import '../src/style.css';

// Test-only route: this fixture never imports Tauri or accesses real cache directories.
const params = new URLSearchParams(location.search);
const locale = params.get('language') === 'en' ? 'en' : 'zh';
const phase = params.get('phase') ?? 'review';
const entry = (id: string, overrides: Partial<Entry> = {}): Entry => ({
  id,
  name: id,
  path: `/test/cache/${id}`,
  group: 'packages',
  bytes: 120_000_000,
  files: 100,
  errors: 0,
  blocked: [],
  device: 1,
  inode: 1,
  isDirectory: true,
  ...overrides,
});
const entries = [
  entry('npm'),
  entry('pip'),
  entry('go'),
  entry('playwright'),
  entry('chrome', { blocked: ['Google Chrome'] }),
  entry('electron', { errors: 1 }),
  entry('electron-builder', { bytes: 0 }),
];
function result(ids: string[]): BatchReport {
  return {
    items: ids.slice(0, 2).map((id, i) => ({ id, error: i === 0 ? null : 'target_busy' })),
    cancelled: ids.length > 2,
    availableChange: 100_000_000,
    persistenceError: null,
  };
}
const state = $state({
  report: phase === 'results' ? result(['npm', 'pip']) : (null as BatchReport | null),
  cleaning: phase === 'running',
});
const copy = messages[locale];
const app = mount(SmartCleanup, {
  target: document.getElementById('fixture')!,
  props: {
    entries,
    ready: phase !== 'scanning',
    scanning: phase === 'scanning',
    get cleaning() {
      return state.cleaning;
    },
    stopping: false,
    progress: { id: 'npm', completed: 1, total: 3 },
    scanDetail: '1,024 entries · /test/cache',
    get report() {
      return state.report;
    },
    error: null,
    t: (key: string, values: Record<string, string | number> = {}) => {
      let text = copy[key as MessageKey] ?? key;
      for (const [name, value] of Object.entries(values))
        text = text.replaceAll(`{${name}}`, String(value));
      return text;
    },
    bytes: (value: number | undefined, signed = false) =>
      value === undefined ? '—' : `${signed && value > 0 ? '+' : ''}${(value / 1e6).toFixed(1)} MB`,
    onClean: (ids: string[]) => {
      state.report = result(ids);
    },
    onClose: () => {
      void unmount(app);
      document.body.append('Fixture closed. No files were deleted.');
    },
    onStop: () => {
      state.cleaning = false;
      state.report = result(['npm', 'pip']);
    },
    onRetry: () => {
      location.search = `language=${locale}`;
    },
  },
});
