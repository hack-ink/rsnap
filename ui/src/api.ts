import { invoke } from '@tauri-apps/api/core'

export type SettingsDto = {
  output_dir: string
}

const normalizeResult = (value: string): string => {
  const normalized = value.trim()
  if (!normalized) return ''
  return normalized.includes(',') ? normalized.split(',').at(-1) ?? '' : normalized
}

export const captureNow = async (): Promise<void> => {
  await invoke('capture_now')
}

export const getLastCaptureBase64 = async (): Promise<string> => {
  const result = await invoke<string>('get_last_capture_base64')
  return normalizeResult(result)
}

export const savePngBase64 = async (pngBase64: string): Promise<string> => {
  return invoke<string>('save_png_base64', {
    pngBase64: normalizeResult(pngBase64),
  })
}

export const copyPngBase64 = async (pngBase64: string): Promise<void> => {
  await invoke('copy_png_base64', {
    pngBase64: normalizeResult(pngBase64),
  })
}

export const openPinWindow = async (): Promise<void> => {
  await invoke('open_pin_window')
}

export const getSettings = async (): Promise<SettingsDto> => {
  return invoke<SettingsDto>('get_settings')
}

export const setOutputDir = async (outputDir: string): Promise<SettingsDto> => {
  return invoke<SettingsDto>('set_output_dir', {
    outputDir: outputDir.trim(),
  })
}
