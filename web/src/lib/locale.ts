export type Locale = 'en' | 'zh-CN';
export const LOCALE_KEY = 'diesel-hifi.locale';

export function resolveLocale(saved: string | null, languages: readonly string[]): Locale {
  if (saved === 'en' || saved === 'zh-CN') return saved;
  for (const language of languages) {
    if (/^zh(?:$|-CN\b|-SG\b|-Hans\b)/i.test(language)) return 'zh-CN';
    if (/^en(?:$|-)/i.test(language)) return 'en';
  }
  return 'en';
}

export function initialLocale(): Locale {
  let saved: string | null = null;
  try { saved = localStorage.getItem(LOCALE_KEY); } catch { /* Storage is optional. */ }
  return resolveLocale(saved, typeof navigator === 'undefined' ? [] : navigator.languages);
}
