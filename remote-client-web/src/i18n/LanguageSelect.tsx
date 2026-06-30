import { Select } from 'antd'
import { supportedLanguages, type SupportedLanguage } from './messages.js'
import { useI18n } from './index.js'

const languageOptions: Array<{ value: SupportedLanguage; label: string }> = [
  { value: 'zh-CN', label: '简体中文' },
  { value: 'zh-TW', label: '繁體中文' },
  { value: 'en', label: 'English' },
  { value: 'ja', label: '日本語' },
  { value: 'ko', label: '한국어' },
  { value: 'de', label: 'Deutsch' }
]

export function LanguageSelect() {
  const { language, setLanguage, t } = useI18n()

  return (
    <label className="language-select">
      <span>{t('language_label')}</span>
      <Select
        aria-label={t('language_label')}
        value={language}
        options={languageOptions}
        onChange={(value) => {
          if (supportedLanguages.includes(value)) setLanguage(value)
        }}
      />
    </label>
  )
}
