import { createTranslator, type TranslationKey } from '@relay/i18n'

export type Translator = (key: TranslationKey) => string
export { createTranslator }
