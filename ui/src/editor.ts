import {
  captureNow,
  copyPngBase64,
  getLastCaptureBase64,
  openPinWindow,
  savePngBase64,
} from './api'

type CropRect = {
  x: number
  y: number
  w: number
  h: number
}

type Point = {
  x: number
  y: number
}

type EditorState = {
  image: HTMLImageElement | null
  captureBase64: string | null
  selection: CropRect | null
  isSelecting: boolean
  startPoint: Point
}

const clamp = (value: number, min: number, max: number): number =>
  Math.max(min, Math.min(max, value))

const clampRect = (value: CropRect, maxWidth: number, maxHeight: number): CropRect | null => {
  const x1 = clamp(value.x, 0, maxWidth)
  const y1 = clamp(value.y, 0, maxHeight)
  const x2 = clamp(value.x + value.w, 0, maxWidth)
  const y2 = clamp(value.y + value.h, 0, maxHeight)
  const normalized: CropRect = {
    x: Math.min(x1, x2),
    y: Math.min(y1, y2),
    w: Math.abs(x2 - x1),
    h: Math.abs(y2 - y1),
  }
  return normalized.w >= 1 && normalized.h >= 1
    ? normalized
    : null
}

const toDataUrlBase64 = (dataUrl: string): string => {
  const [header, encoded = ''] = dataUrl.split(',')
  if (!encoded) {
    return header
  }
  return encoded
}

const formatError = (value: unknown): string =>
  value instanceof Error ? value.message : String(value)

const buildMarkup = (): string => `
  <main class="editor">
    <header class="topbar">
      <h1>rsnap</h1>
      <div class="actions">
        <button id="capture-btn" type="button">Capture</button>
        <button id="save-btn" type="button">Save</button>
        <button id="copy-btn" type="button">Copy</button>
        <button id="pin-btn" type="button">Pin</button>
      </div>
    </header>

    <p id="status" class="status" aria-live="polite"></p>

    <section class="canvas-shell">
      <p id="placeholder">No capture loaded. Use Capture to take one.</p>
      <canvas id="capture-canvas" aria-label="capture canvas"></canvas>
    </section>

    <p class="hint">
      Drag on the image to select a crop; Save and Copy will export that area (or full image if no crop).
    </p>
  </main>
`

const createEditor = () => {
  const app = document.querySelector<HTMLDivElement>('#app')
  if (!app) {
    throw new Error('Missing #app root container')
  }

  app.innerHTML = buildMarkup()

  const captureCanvas = document.querySelector<HTMLCanvasElement>('#capture-canvas')
  const placeholder = document.querySelector<HTMLParagraphElement>('#placeholder')
  const statusEl = document.querySelector<HTMLParagraphElement>('#status')
  const captureButton = document.querySelector<HTMLButtonElement>('#capture-btn')
  const saveButton = document.querySelector<HTMLButtonElement>('#save-btn')
  const copyButton = document.querySelector<HTMLButtonElement>('#copy-btn')
  const pinButton = document.querySelector<HTMLButtonElement>('#pin-btn')

  if (
    !captureCanvas ||
    !placeholder ||
    !statusEl ||
    !captureButton ||
    !saveButton ||
    !copyButton ||
    !pinButton
  ) {
    throw new Error('Failed to build editor controls')
  }

  const ctx = captureCanvas.getContext('2d')
  if (!ctx) {
    throw new Error('Failed to get 2d canvas context')
  }

  const state: EditorState = {
    image: null,
    captureBase64: null,
    selection: null,
    isSelecting: false,
    startPoint: { x: 0, y: 0 },
  }

  const setStatus = (message: string): void => {
    statusEl.textContent = message
  }

  const setBusy = (value: boolean): void => {
    captureButton.disabled = value
    saveButton.disabled = value
    copyButton.disabled = value
    pinButton.disabled = value
  }

  const render = (): void => {
    if (!state.image) {
      captureCanvas.classList.remove('has-image')
      placeholder.hidden = false
      return
    }

    placeholder.hidden = true
    captureCanvas.classList.add('has-image')
    const width = state.image.naturalWidth
    const height = state.image.naturalHeight
    const dpr = Math.max(1, Math.floor(window.devicePixelRatio || 1))
    captureCanvas.width = width * dpr
    captureCanvas.height = height * dpr
    captureCanvas.style.aspectRatio = `${width} / ${height}`
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0)
    ctx.clearRect(0, 0, width, height)
    ctx.drawImage(state.image, 0, 0, width, height)

    if (state.selection) {
      const { x, y, w, h } = state.selection
      ctx.fillStyle = 'rgba(0, 0, 0, 0.35)'
      ctx.fillRect(0, 0, width, y)
      ctx.fillRect(0, y, x, h)
      ctx.fillRect(x + w, y, width - (x + w), h)
      ctx.fillRect(0, y + h, width, height - (y + h))

      ctx.strokeStyle = '#ffffff'
      ctx.lineWidth = 2
      ctx.setLineDash([6, 4])
      ctx.strokeRect(x + 1, y + 1, w - 2, h - 2)
      ctx.setLineDash([])
      ctx.strokeStyle = '#93c5fd'
      ctx.strokeRect(x, y, w, h)
    }
  }

  const toImagePoint = (event: PointerEvent): Point => {
    const bounds = captureCanvas.getBoundingClientRect()
    const x = ((event.clientX - bounds.left) * (state.image?.naturalWidth ?? 1)) / bounds.width
    const y = ((event.clientY - bounds.top) * (state.image?.naturalHeight ?? 1)) / bounds.height
    return {
      x: clamp(x, 0, state.image?.naturalWidth ?? 0),
      y: clamp(y, 0, state.image?.naturalHeight ?? 0),
    }
  }

  const getExportBase64 = (): string | null => {
    if (!state.image) return null
    const source = state.selection ?? {
      x: 0,
      y: 0,
      w: state.image.naturalWidth,
      h: state.image.naturalHeight,
    }
    if (!Number.isFinite(source.x + source.y + source.w + source.h)) return null

    const out = document.createElement('canvas')
    out.width = Math.max(1, Math.round(source.w))
    out.height = Math.max(1, Math.round(source.h))
    const outCtx = out.getContext('2d')
    if (!outCtx) return null

    outCtx.drawImage(
      state.image,
      source.x,
      source.y,
      source.w,
      source.h,
      0,
      0,
      out.width,
      out.height
    )
    return toDataUrlBase64(out.toDataURL('image/png'))
  }

  const loadCapture = async (base64: string): Promise<void> => {
    if (!base64) {
      state.image = null
      state.captureBase64 = null
      state.selection = null
      setStatus('No capture found yet.')
      render()
      return
    }

    return new Promise((resolve, reject) => {
      const image = new Image()
      image.onload = () => {
        state.image = image
        state.captureBase64 = base64
        state.selection = null
        render()
        setStatus('')
        resolve()
      }
      image.onerror = () => {
        reject(new Error('Invalid image data from backend'))
      }
      image.src = `data:image/png;base64,${base64}`
    })
  }

  const refreshLastCapture = async (): Promise<void> => {
    const result = await getLastCaptureBase64()
    await loadCapture(result)
  }

  const finalizeSelection = (): void => {
    if (!state.selection || !state.image) return
    const bounded = clampRect(
      state.selection,
      state.image.naturalWidth,
      state.image.naturalHeight
    )
    state.selection = bounded
    if (bounded && bounded.w <= 2 && bounded.h <= 2) {
      state.selection = null
    }
    render()
  }

  captureCanvas.addEventListener('pointerdown', (event) => {
    if (!state.image) return
    state.isSelecting = true
    const point = toImagePoint(event)
    state.startPoint = point
    state.selection = {
      x: point.x,
      y: point.y,
      w: 0,
      h: 0,
    }
    captureCanvas.setPointerCapture(event.pointerId)
    render()
  })

  captureCanvas.addEventListener('pointermove', (event) => {
    if (!state.isSelecting || !state.selection || !state.image) return
    const point = toImagePoint(event)
    state.selection = {
      x: state.startPoint.x,
      y: state.startPoint.y,
      w: point.x - state.startPoint.x,
      h: point.y - state.startPoint.y,
    }
    render()
  })

  const endSelection = (): void => {
    if (!state.isSelecting) return
    state.isSelecting = false
    finalizeSelection()
  }

  captureCanvas.addEventListener('pointerup', () => endSelection())
  captureCanvas.addEventListener('pointercancel', () => endSelection())
  captureCanvas.addEventListener('pointerleave', () => endSelection())

  captureButton.addEventListener('click', async () => {
    try {
      setBusy(true)
      setStatus('Capturing...')
      await captureNow()
      await refreshLastCapture()
      setStatus('Capture ready')
    } catch (error) {
      setStatus(`Capture failed: ${formatError(error)}`)
    } finally {
      setBusy(false)
    }
  })

  saveButton.addEventListener('click', async () => {
    try {
      const pngBase64 = getExportBase64()
      if (!pngBase64) {
        setStatus('Load a capture before saving.')
        return
      }
      setBusy(true)
      setStatus('Saving...')
      const savedPath = await savePngBase64(pngBase64)
      setStatus(`Saved: ${savedPath}`)
    } catch (error) {
      setStatus(`Save failed: ${formatError(error)}`)
    } finally {
      setBusy(false)
    }
  })

  copyButton.addEventListener('click', async () => {
    try {
      const pngBase64 = getExportBase64()
      if (!pngBase64) {
        setStatus('Load a capture before copying.')
        return
      }
      setBusy(true)
      setStatus('Copying...')
      await copyPngBase64(pngBase64)
      setStatus('Copied')
    } catch (error) {
      setStatus(`Copy failed: ${formatError(error)}`)
    } finally {
      setBusy(false)
    }
  })

  pinButton.addEventListener('click', async () => {
    try {
      setBusy(true)
      await openPinWindow()
      setStatus('Pin window requested')
    } catch (error) {
      setStatus(`Pin failed: ${formatError(error)}`)
    } finally {
      setBusy(false)
    }
  })

  setBusy(false)

  return {
    load: refreshLastCapture,
  }
}

export const initEditor = async (): Promise<void> => {
  const editor = createEditor()
  try {
    await editor.load()
  } catch (error) {
    // backend not yet captured or available yet
    const status = document.querySelector<HTMLParagraphElement>('#status')
    if (status) {
      status.textContent = `Unable to load capture: ${formatError(error)}`
    }
  }
}
