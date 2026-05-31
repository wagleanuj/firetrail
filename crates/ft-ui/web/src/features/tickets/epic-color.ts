/** Deterministic, stable color for an epic id. No persistence — pure hash → hue. */
export function epicColor(epicId: string): string {
  let h = 0
  for (let i = 0; i < epicId.length; i++) {
    h = (h * 31 + epicId.charCodeAt(i)) >>> 0
  }
  const hue = h % 360
  return `hsl(${hue} 65% 60%)`
}
