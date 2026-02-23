import { getSettings, setOutputDir } from './api'

const formatError = (value: unknown): string =>
  value instanceof Error ? value.message : String(value)

const buildMarkup = (): string => `
  <main class="editor">
    <header class="topbar">
      <h1>Settings</h1>
      <div class="actions">
        <button id="save-btn" type="button">Save</button>
      </div>
    </header>

    <p id="status" class="status" aria-live="polite"></p>

    <section class="canvas-shell">
      <label class="hint" for="output-dir">Default save directory</label>
      <input id="output-dir" class="input" type="text" spellcheck="false" />
      <p class="hint">
        Default is Desktop. You can set any writable folder path. Use <code>~</code> for your home.
      </p>
    </section>
  </main>
`

export const initSettings = async (): Promise<void> => {
  const app = document.querySelector<HTMLDivElement>('#app')
  if (!app) throw new Error('Missing #app root container')

  app.innerHTML = buildMarkup()

  const statusEl = document.querySelector<HTMLParagraphElement>('#status')
  const outputDirInput = document.querySelector<HTMLInputElement>('#output-dir')
  const saveButton = document.querySelector<HTMLButtonElement>('#save-btn')

  if (!statusEl || !outputDirInput || !saveButton) {
    throw new Error('Failed to build settings controls')
  }

  const setStatus = (message: string): void => {
    statusEl.textContent = message
  }

  const setBusy = (value: boolean): void => {
    saveButton.disabled = value
    outputDirInput.disabled = value
  }

  try {
    setBusy(true)
    const settings = await getSettings()
    outputDirInput.value = settings.output_dir
    setStatus('')
  } catch (error) {
    setStatus(`Failed to load settings: ${formatError(error)}`)
  } finally {
    setBusy(false)
  }

  saveButton.addEventListener('click', async () => {
    try {
      setBusy(true)
      setStatus('Saving...')
      const settings = await setOutputDir(outputDirInput.value)
      outputDirInput.value = settings.output_dir
      setStatus('Saved')
    } catch (error) {
      setStatus(`Save failed: ${formatError(error)}`)
    } finally {
      setBusy(false)
    }
  })
}

