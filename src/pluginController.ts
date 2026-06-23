export function pluginActionKey(pluginId: string, actionId: string) {
  return `${pluginId}:${actionId}`
}

export function delay(ms: number) {
  return new Promise<void>((resolve) => window.setTimeout(resolve, ms))
}

export function collectPluginConfig(form: HTMLFormElement) {
  const config: Record<string, unknown> = {}
  form.querySelectorAll<HTMLInputElement | HTMLSelectElement>('[data-plugin-config-field]').forEach(
    (input) => {
      const key = input.dataset.pluginConfigField
      if (!key) {
        return
      }
      if (input instanceof HTMLInputElement && input.type === 'checkbox') {
        config[key] = input.checked
      } else if (input instanceof HTMLInputElement && input.type === 'number') {
        config[key] = input.value === '' ? null : Number(input.value)
      } else {
        config[key] = input.value
      }
    }
  )
  return config
}

export function cssEscape(value: string) {
  return typeof CSS !== 'undefined' && CSS.escape ? CSS.escape(value) : value.replace(/"/g, '\\"')
}
