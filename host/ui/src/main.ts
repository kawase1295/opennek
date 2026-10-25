// The OpenNEK web UI
import { mount } from 'svelte'
import App from './App.svelte'
import { api } from './lib/api'
import { chooseLang, setLang, takeLegacyLang } from './lib/i18n'
import { applyTheme } from './lib/theme'
import './style.css'

// The language and the theme come from the settings before anything is shown.
// A language left in localStorage by an older version moves to the settings once
async function loadPreferences(): Promise<void> {
  const settings = await api.settings().catch(() => null)
  applyTheme(settings?.theme ?? 'system')
  const choice = chooseLang(settings?.language, takeLegacyLang(), navigator.languages)
  setLang(choice.lang)
  document.documentElement.lang = choice.lang
  if (choice.migrate) await api.saveSettings({ language: choice.lang }).catch(() => {})
}

await loadPreferences()
mount(App, { target: document.body })
