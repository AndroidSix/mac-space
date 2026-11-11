import { test } from 'node:test';
import assert from 'node:assert/strict';
import { cleanupBlock, recommendedIds } from '../src/smart-cleanup.ts';
import type { Entry } from '../src/types.ts';

test('smart selection excludes active, incomplete, empty, rebuild and runtime caches', () => {
  const entry = (id: string, overrides: Partial<Entry> = {}): Entry => ({
    id,
    name: id,
    path: '/test',
    group: 'packages',
    bytes: 4096,
    files: 1,
    errors: 0,
    blocked: [],
    device: 1,
    inode: 1,
    isDirectory: true,
    ...overrides,
  });
  const entries = [
    entry('pip'),
    entry('npm', { blocked: ['npm'] }),
    entry('electron', { errors: 1 }),
    entry('electron-builder', { bytes: 0 }),
    entry('vscode-update'),
    entry('xcode'),
    entry('go'),
    entry('uv'),
    entry('chrome'),
    entry('edge'),
    entry('playwright'),
    entry('unknown'),
    entry('toString'),
  ];
  assert.deepEqual(recommendedIds(entries), ['pip', 'vscode-update']);
  assert.equal(cleanupBlock(entries[1]), 'target_busy');
  assert.equal(cleanupBlock(entries[2]), 'scan_incomplete');
  assert.equal(cleanupBlock(entries[3]), 'empty');
  assert.equal(cleanupBlock(entries[11]), 'invalid_target');
  assert.equal(cleanupBlock(entries[12]), 'invalid_target');
});
