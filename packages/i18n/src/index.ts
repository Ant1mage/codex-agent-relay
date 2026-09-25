import type { Locale } from '@relay/protocol'

export const defaultLocale: Locale = 'en'
export const supportedLocales: readonly Locale[] = ['en', 'zh-CN']

const en = {
  'app.name': 'Relay',
  'nav.sessions': 'Sessions',
  'nav.agents': 'Agents',
  'nav.settings': 'Settings',
  'app.tagline': 'Subagent control plane',
  'sessions.title': 'Sessions',
  'sessions.subtitle': 'Codex sessions and delegated runs',
  'sessions.empty': 'No Codex sessions yet',
  'sessions.emptyHint': 'Start a Relay delegation from Codex to see it here.',
  'runs.title': 'Runs',
  'runs.empty': 'No runs in this session',
  'runs.events': 'Live events',
  'runs.duration': 'Duration',
  'runs.worker': 'Worker',
  'agents.title': 'Agents',
  'agents.subtitle': 'Detected runtimes and profiles exposed to Codex',
  'agents.empty': 'No agent runtimes detected',
  'agents.available': 'Available',
  'agents.authRequired': 'Authentication required',
  'agents.create': 'Create profile',
  'agents.edit': 'Edit profile',
  'agents.name': 'Name',
  'agents.description': 'Description',
  'agents.enabled': 'Enabled',
  'agents.read': 'Read workspace',
  'agents.write': 'Write workspace',
  'agents.shell': 'Execute commands',
  'agents.network': 'Network access',
  'agents.profileSaved': 'Profile saved',
  'settings.title': 'Settings',
  'settings.subtitle': 'Language and conservative routing defaults',
  'settings.policy': 'Routing policy',
  'settings.maxRuns': 'Maximum concurrent runs',
  'settings.maxWriters': 'Maximum concurrent writers',
  'settings.requireWorktree': 'Require worktrees for parallel writers',
  'settings.saved': 'Settings saved',
  'settings.scope': 'Policy scope',
  'settings.global': 'Global defaults',
  'settings.workspace': 'Workspace override',
  'settings.allowWrite': 'Allow workspace writes',
  'settings.allowCommands': 'Allow command execution',
  'settings.allowNetwork': 'Allow network access',
  'runs.cancelQueued': 'Cancellation requested',
  'common.active': 'Active',
  'common.ended': 'Ended',
  'common.offline': 'Offline',
  'common.refresh': 'Refresh',
  'common.notAvailable': 'Not available',
  'run.status.queued': 'Queued',
  'run.status.starting': 'Starting',
  'run.status.running': 'Running',
  'run.status.completed': 'Completed',
  'run.status.failed': 'Failed',
  'run.status.cancelled': 'Cancelled',
  'run.status.handed_off': 'Handed off',
  'action.cancel': 'Cancel',
  'action.save': 'Save',
  'action.close': 'Close',
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
  'app.tagline': '子智能体控制平面',
  'sessions.title': '会话',
  'sessions.subtitle': 'Codex 会话与委派任务',
  'sessions.empty': '还没有 Codex 会话',
  'sessions.emptyHint': '从 Codex 发起一次 Relay 委派后，会话会显示在这里。',
  'runs.title': '运行任务',
  'runs.empty': '此会话还没有运行任务',
  'runs.events': '实时事件',
  'runs.duration': '耗时',
  'runs.worker': 'Worker',
  'agents.title': '智能体',
  'agents.subtitle': '已检测的 Runtime 与暴露给 Codex 的 Profile',
  'agents.empty': '未检测到智能体 Runtime',
  'agents.available': '可用',
  'agents.authRequired': '需要认证',
  'agents.create': '创建 Profile',
  'agents.edit': '编辑 Profile',
  'agents.name': '名称',
  'agents.description': '说明',
  'agents.enabled': '启用',
  'agents.read': '读取工作区',
  'agents.write': '写入工作区',
  'agents.shell': '执行命令',
  'agents.network': '访问网络',
  'agents.profileSaved': 'Profile 已保存',
  'settings.title': '设置',
  'settings.subtitle': '语言与保守的路由默认值',
  'settings.policy': '路由策略',
  'settings.maxRuns': '最大并发任务数',
  'settings.maxWriters': '最大并发写入数',
  'settings.requireWorktree': '并行写入时要求 worktree',
  'settings.saved': '设置已保存',
  'settings.scope': '策略范围',
  'settings.global': '全局默认值',
  'settings.workspace': '工作区覆盖',
  'settings.allowWrite': '允许写入工作区',
  'settings.allowCommands': '允许执行命令',
  'settings.allowNetwork': '允许访问网络',
  'runs.cancelQueued': '已请求取消任务',
  'common.active': '活跃',
  'common.ended': '已结束',
  'common.offline': '离线',
  'common.refresh': '刷新',
  'common.notAvailable': '不可用',
  'run.status.queued': '排队中',
  'run.status.starting': '启动中',
  'run.status.running': '运行中',
  'run.status.completed': '已完成',
  'run.status.failed': '失败',
  'run.status.cancelled': '已取消',
  'run.status.handed_off': '已交接',
  'action.cancel': '取消',
  'action.save': '保存',
  'action.close': '关闭',
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
