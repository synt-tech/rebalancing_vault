import { useState } from 'react'

/**
 * Small "copy" button with transient 'copied' feedback.
 * Shared between TxSubmitModal's error panel and error Toasts.
 * @param {string}  props.text        - text written to the clipboard
 * @param {string}  props.label       - idle label (default 'copy')
 * @param {string}  props.copiedLabel - shown briefly after a successful copy
 * @param {string}  props.className   - styling hook (sizing/colors live with the caller)
 * @param {string}  props.title       - tooltip
 */
export default function CopyButton({ text, label = 'copy', copiedLabel = 'copied', className = '', title = 'copy to clipboard' }) {
  const [copied, setCopied] = useState(false)

  const copy = async () => {
    try {
      await navigator.clipboard.writeText(text || '')
      setCopied(true)
      setTimeout(() => setCopied(false), 1500)
    } catch (e) {
      console.error('copy failed:', e)
    }
  }

  return (
    <button onClick={copy} className={className} title={title}>
      {copied ? copiedLabel : label}
    </button>
  )
}
