export async function copyJson(value: unknown): Promise<void> {
  const text = JSON.stringify(value, null, 2)
  // navigator.clipboard requires a secure context (https or localhost); this
  // app is also served over plain http on the LAN, so fall back to the
  // legacy execCommand copy path when the async API isn't available.
  if (navigator.clipboard) {
    await navigator.clipboard.writeText(text)
  } else {
    const textarea = document.createElement('textarea')
    textarea.value = text
    textarea.style.position = 'fixed'
    textarea.style.opacity = '0'
    document.body.appendChild(textarea)
    textarea.select()
    document.execCommand('copy')
    document.body.removeChild(textarea)
  }
}

