import { initialLocale, LOCALE_KEY, type Locale } from './locale';
import { en, zhCN, type MessageKey } from './messages';

class LanguageStore {
  locale = $state<Locale>(initialLocale());

  set(locale: Locale): void {
    this.locale = locale;
    try { localStorage.setItem(LOCALE_KEY, locale); } catch { /* Keep working without storage. */ }
  }

  t(key: MessageKey, values: Record<string, string | number> = {}): string {
    const message = (this.locale === 'zh-CN' ? zhCN : en)[key];
    return message.replace(/\{(\w+)\}/g, (match, name: string) => String(values[name] ?? match));
  }
}

export const language = new LanguageStore();
