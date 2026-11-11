import { test } from 'node:test';
import assert from 'node:assert/strict';
import { messages } from '../src/i18n.ts';

test('English and Chinese cover every message and preserve interpolation fields', () => {
  assert.deepEqual(Object.keys(messages.zh).sort(), Object.keys(messages.en).sort());
  for (const key of Object.keys(messages.en) as (keyof typeof messages.en)[]) {
    assert.ok(messages.en[key].trim() && messages.zh[key].trim(), key);
    const fields = (value: string) =>
      [...value.matchAll(/\{(\w+)\}/g)].map((match) => match[1]).sort();
    assert.deepEqual(fields(messages.en[key]), fields(messages.zh[key]), key);
  }
});
