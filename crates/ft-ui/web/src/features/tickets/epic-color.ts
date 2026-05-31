/** Deterministic, stable color for an epic id. No persistence — pure hash → hue. */

function hueFor(epicId: string): number {
  let h = 0
  for (let i = 0; i < epicId.length; i++) {
    h = (h * 31 + epicId.charCodeAt(i)) >>> 0
  }
  return h % 360
}

export function epicColor(epicId: string): string {
  return `hsl(${hueFor(epicId)} 65% 60%)`
}

/** Translucent background tint for an epic id (same hue as epicColor). */
export function epicColorSoft(epicId: string): string {
  return `hsl(${hueFor(epicId)} 65% 60% / 0.13)`
}
