import { describe, expect, it } from 'vitest'
import { createTranslator, resolveLocale, supportedLocales } from '../src/index.js'

describe('i18n', () => {
  it('supports only English and Simplified Chinese', () => {
    expect(supportedLocales).toEqual(['en', 'zh-CN'])
    expect(resolveLocale('zh-Hans-CN')).toBe('zh-CN')
    expect(resolveLocale('en-US')).toBe('en')
    expect(resolveLocale('fr-FR')).toBe('en')
  })

  it('translates shared interface vocabulary', () => {
    expect(createTranslator('en')('nav.sessions')).toBe('Sessions')
    expect(createTranslator('zh-CN')('nav.sessions')).toBe('会话')
    expect(createTranslator('en')('settings.fontSize')).toBe('Font size')
    expect(createTranslator('zh-CN')('settings.fontSize')).toBe('字号')
  })
})
