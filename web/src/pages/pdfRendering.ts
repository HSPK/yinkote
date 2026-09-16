// A replacement bitmap can coexist with the visible one while it paints.
export const MAX_CANVAS_PIXELS = 8_388_608
export const MAX_CANVAS_DIMENSION = 8192

export function canvasOutput(width: number, height: number, deviceRatio: number) {
  if (!Number.isFinite(width) || !Number.isFinite(height) || width <= 0 || height <= 0) {
    throw new Error('Invalid PDF page dimensions')
  }
  const ratio = Number.isFinite(deviceRatio) && deviceRatio > 0 ? deviceRatio : 1
  const scale = Math.min(
    ratio,
    MAX_CANVAS_DIMENSION / width,
    MAX_CANVAS_DIMENSION / height,
    Math.sqrt(MAX_CANVAS_PIXELS) / Math.sqrt(width) / Math.sqrt(height),
  )
  const pixelsWide = Math.max(1, Math.floor(width * scale))
  const pixelsHigh = Math.max(1, Math.floor(height * scale))
  return {
    width: pixelsWide,
    height: pixelsHigh,
    scaleX: pixelsWide / width,
    scaleY: pixelsHigh / height,
  }
}
