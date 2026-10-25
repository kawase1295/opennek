// The OpenNEK web UI. The language is read from the settings before anything is shown
import { mount } from 'svelte'
import App from './App.svelte'
import { api } from './lib/api'
import { chooseLang, setLang, takeLegacyLang } from './lib/i18n'
import './style.css'

// a language left in localStorage by an older version moves to the settings once
async function loadLang(): Promise<void> {
  const saved = await api.settings().then(
    (s) => s.language,
    () => undefined,
  )
  const choice = chooseLang(saved, takeLegacyLang(), navigator.languages)
  setLang(choice.lang)
  document.documentElement.lang = choice.lang
  if (choice.migrate) await api.saveSettings({ language: choice.lang }).catch(() => {})
}

await loadLang()
mount(App, { target: document.body })
