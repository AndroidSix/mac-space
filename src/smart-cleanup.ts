import type { Entry } from './types';

const impacts: Record<string, { hint: string; recommended: boolean }> = {
  pip: { hint: 'impactDownload', recommended: true },
  npm: { hint: 'impactDownload', recommended: true },
  electron: { hint: 'impactDownload', recommended: true },
  'electron-builder': { hint: 'impactDownload', recommended: true },
  'vscode-update': { hint: 'impactUpdate', recommended: true },
  xcode: { hint: 'impactBuild', recommended: false },
  go: { hint: 'impactBuild', recommended: false },
  uv: { hint: 'impactUv', recommended: false },
  chrome: { hint: 'impactBrowser', recommended: false },
  edge: { hint: 'impactBrowser', recommended: false },
  playwright: { hint: 'impactPlaywright', recommended: false },
};

export function cleanupImpact(entry: Entry) {
  return Object.hasOwn(impacts, entry.id) ? impacts[entry.id] : undefined;
}

export function cleanupBlock(entry: Entry): string | null {
  if (!Object.hasOwn(impacts, entry.id)) return 'invalid_target';
  if (entry.errors) return 'scan_incomplete';
  if (entry.blocked.length) return 'target_busy';
  if (entry.bytes < 1024) return 'empty';
  return null;
}

export function recommendedIds(entries: Entry[]): string[] {
  return entries
    .filter((entry) => !cleanupBlock(entry) && cleanupImpact(entry)?.recommended)
    .map((entry) => entry.id);
}
