import type { Locale } from '@relay/protocol'

export const defaultLocale: Locale = 'en'
export const supportedLocales: readonly Locale[] = ['en', 'zh-CN']

const en = {
  'app.name': 'Relay',
  'nav.sessions': 'Sessions',
  'nav.agents': 'Agents',
  'nav.settings': 'Settings',
  'run.status.queued': 'Queued',
  'run.status.starting': 'Starting',
  'run.status.running': 'Running',
  'run.status.completed': 'Completed',
  'run.status.failed': 'Failed',
  'run.status.cancelled': 'Cancelled',
  'run.status.handed_off': 'Handed off',
  'action.cancel': 'Cancel',
  'action.save': 'Save',
  'settings.language': 'Language',
  'language.en': 'English',
  'language.zh-CN': 'Simplified Chinese',
  'error.generic': 'Something went wrong.',
} as const

type MessageKey = keyof typeof en
type Messages = Record<MessageKey, string>

const zhCN: Messages = {
  'app.name': 'Relay',
  'nav.sessions': '会话',
  'nav.agents': '智能体',
  'nav.settings': '设置',
  'run.status.queued': '排队中',
  'run.status.starting': '启动中',
  'run.status.running': '运行中',
  'run.status.completed': '已完成',
  'run.status.failed': '失败',
  'run.status.cancelled': '已取消',
  'run.status.handed_off': '已交接',
  'action.cancel': '取消',
  'action.save': '保存',
  'settings.language': '语言',
  'language.en': '英语',
  'language.zh-CN': '简体中文',
  'error.generic': '发生了错误。',
}

const resources: Record<Locale, Messages> = { en, 'zh-CN': zhCN }

export type TranslationKey = MessageKey

export function resolveLocale(language: string | undefined): Locale {
  if (language?.toLowerCase().startsWith('zh')) return 'zh-CN'
  return defaultLocale
}

export function translate(locale: Locale, key: TranslationKey): string {
  return resources[locale][key] ?? resources[defaultLocale][key]
}

export function createTranslator(locale: Locale): (key: TranslationKey) => string {
  return (key) => translate(locale, key)
}

