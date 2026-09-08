import { describe, expect, it } from 'vitest';
import { resolveLocale } from '../src/lib/locale';
import { en, zhCN } from '../src/lib/messages';

describe('language selection', () => {
  it('prefers a saved supported language over the browser language', () => {
    expect(resolveLocale('en', ['zh-CN'])).toBe('en');
    expect(resolveLocale('zh-CN', ['en-US'])).toBe('zh-CN');
  });
  it('detects Simplified Chinese and follows browser preference order', () => {
    for (const code of ['zh', 'zh-CN', 'zh-SG', 'zh-Hans', 'zh-Hans-CN']) {
      expect(resolveLocale(null, [code])).toBe('zh-CN');
    }
    expect(resolveLocale(null, ['fr-FR', 'zh-CN', 'en-US'])).toBe('zh-CN');
    expect(resolveLocale(null, ['en-GB', 'zh-CN'])).toBe('en');
  });
  it('falls back for unsupported saved and browser languages', () => {
    expect(resolveLocale('invalid', ['zh-CN'])).toBe('zh-CN');
    expect(resolveLocale(null, ['zh-Hant-TW', 'fr-FR'])).toBe('en');
    expect(resolveLocale(null, [])).toBe('en');
  });
  it('ships complete Chinese messages with matching substitutions', () => {
    expect(Object.keys(zhCN).sort()).toEqual(Object.keys(en).sort());
    for (const key of Object.keys(en) as (keyof typeof en)[]) {
      expect(zhCN[key].trim(), key).not.toBe('');
      expect(zhCN[key].match(/\{\w+\}/g) ?? [], key).toEqual(en[key].match(/\{\w+\}/g) ?? []);
    }
  });
});
