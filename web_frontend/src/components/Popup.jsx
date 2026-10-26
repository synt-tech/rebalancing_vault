import { useEffect, useRef, useState } from 'react'
import { createPortal } from 'react-dom'

/**
 * Unified popup/modal template with pixelated backdrop overlay.
 * On open, paints a 1/5-scale snapshot of the page onto a small canvas
 * (background rects, chart canvases, same-origin images, text runs), then
 * CSS image-rendering: pixelated upscales it into true 5x5px blocky
 * subsampling. Runs in ~100ms with no dependencies; falls back to a
 * transparent overlay on failure.
 *
 * @param {boolean} props.open - whether the popup is visible
 * @param {Function} props.onClose - called when overlay is clicked or close button pressed
 * @param {React.ReactNode} [props.title] - optional header title
 * @param {React.ReactNode} [props.children] - popup content
 * @param {string} [props.maxWidth='max-w-2xl'] - max width class for content
 * @param {string} [props.maxHeight='max-h-[85vh]'] - max height class for content
 * @param {boolean} [props.fitContent=false] - size width to content (up to 95vw) instead of maxWidth
 * @param {React.RefObject} [props.anchorRef] - if provided, positions popup below this element instead of centering
 * @param {string} [props.width] - width class when anchored (e.g. 'w-[32rem]')
 * @param {boolean} [props.showCloseButton=true] - whether to show the close button
 * @param {React.RefObject} [props.contentRef] - ref to attach to the content container
 */
export default function Popup({
  open,
  onClose,
  title,
  children,
  maxWidth = 'max-w-2xl',
  maxHeight = 'max-h-[85vh]',
  fitContent = false,
  anchorRef = null,
  width = 'w-[32rem]',
  showCloseButton = true,
  contentRef = null,
}) {
  const popupRef = useRef(null)
  const [anchorPos, setAnchorPos] = useState(null)
  const [pixelBg, setPixelBg] = useState(null)

  useEffect(() => {
    if (!open || !anchorRef?.current) return
    const updatePos = () => {
      const rect = anchorRef.current.getBoundingClientRect()
      setAnchorPos({
        left: rect.left,
        top: rect.bottom + 4,
        width: rect.width,
      })
    }
    updatePos()
    window.addEventListener('scroll', updatePos, true)
    window.addEventListener('resize', updatePos)
    return () => {
      window.removeEventListener('scroll', updatePos, true)
      window.removeEventListener('resize', updatePos)
    }
  }, [open, anchorRef])

  useEffect(() => {
    if (!open) return
    const handleKey = (e) => {
      if (e.key === 'Escape') onClose()
    }
    document.addEventListener('keydown', handleKey)
    // Lock background scroll while popup is open
    const prevOverflow = document.body.style.overflow
    document.body.style.overflow = 'hidden'
    return () => {
      document.removeEventListener('keydown', handleKey)
      document.body.style.overflow = prevOverflow
    }
  }, [open, onClose])

  useEffect(() => {
    if (!open) {
      setPixelBg(null)
      return
    }
    let cancelled = false
    const capture = () => {
      try {
        const BLOCK_SIZE = 5
        const s = 1 / BLOCK_SIZE
        const vw = window.innerWidth
        const vh = window.innerHeight
        const out = document.createElement('canvas')
        out.width = Math.ceil(vw * s)
        out.height = Math.ceil(vh * s)
        const ctx = out.getContext('2d')

        const bodyBg = window.getComputedStyle(document.body).backgroundColor
        ctx.fillStyle = bodyBg && bodyBg !== 'rgba(0, 0, 0, 0)' ? bodyBg : '#0a0a0a'
        ctx.fillRect(0, 0, out.width, out.height)

        const root = document.getElementById('root') || document.body
        const origin = window.location.origin

        for (const el of root.querySelectorAll('*')) {
          const tag = el.tagName
          if (tag === 'SCRIPT' || tag === 'STYLE') continue
          const r = el.getBoundingClientRect()
          if (r.width < 1 || r.height < 1) continue
          if (r.bottom < 0 || r.right < 0 || r.top > vh || r.left > vw) continue
          const cs = window.getComputedStyle(el)
          if (cs.visibility === 'hidden' || cs.display === 'none' || parseFloat(cs.opacity) === 0) continue

          let clippedByAncestor = false
          let parent = el.parentElement
          while (parent && parent !== root) {
            const pcs = parent._pcs || (parent._pcs = window.getComputedStyle(parent))
            const ov = pcs.overflowY
            if (ov === 'hidden' || ov === 'auto' || ov === 'scroll') {
              const pr = parent._rect || (parent._rect = parent.getBoundingClientRect())
              if (r.bottom < pr.top || r.top > pr.bottom) {
                clippedByAncestor = true
                break
              }
            }
            const ovx = pcs.overflowX
            if (ovx === 'hidden' || ovx === 'auto' || ovx === 'scroll') {
              const pr = parent._rect || (parent._rect = parent.getBoundingClientRect())
              if (r.right < pr.left || r.left > pr.right) {
                clippedByAncestor = true
                break
              }
            }
            parent = parent.parentElement
          }
          if (clippedByAncestor) continue

          const x = r.left * s
          const y = r.top * s
          const w = r.width * s
          const h = r.height * s

          const bg = cs.backgroundColor
          if (bg && bg !== 'rgba(0, 0, 0, 0)' && bg !== 'transparent') {
            const alphaMatch = bg.match(/rgba?\([^,)]*,\s*([^,)]+)\s*\)/)
            const alpha = alphaMatch ? parseFloat(alphaMatch[1]) : 1
            if (alpha >= 0.5) {
              ctx.fillStyle = bg
              ctx.fillRect(x, y, w, h)
            }
          }

          if (tag === 'CANVAS') {
            try {
              ctx.drawImage(el, x, y, w, h)
            } catch (e) { /* tainted or zero-size canvas */ }
          } else if (tag === 'IMG' && el.complete && el.naturalWidth > 0) {
            const src = el.src || ''
            if (!src.startsWith('http') || src.startsWith(origin)) {
              try {
                ctx.drawImage(el, x, y, w, h)
              } catch (e) { /* tainted image */ }
            }
          }

          for (const child of el.childNodes) {
            if (child.nodeType === 3 && child.textContent.trim()) {
              const fs = parseFloat(cs.fontSize) * s
              ctx.fillStyle = cs.color
              ctx.font = `${cs.fontWeight} ${fs}px ${cs.fontFamily.split(',')[0]}`
              ctx.textBaseline = 'top'
              ctx.fillText(child.textContent.trim().slice(0, 80), x, y + (h - fs) / 2)
              break
            }
          }
        }

        if (!cancelled) setPixelBg(out.toDataURL('image/png'))
      } catch (e) {
        console.error('Pixelation capture failed:', e)
      }
    }
    const raf = requestAnimationFrame(capture)
    return () => {
      cancelled = true
      cancelAnimationFrame(raf)
    }
  }, [open])

  if (!open) return null

  const overlayStyle = pixelBg
    ? {
        backgroundImage: `url(${pixelBg})`,
        backgroundSize: '100% 100%',
        backgroundRepeat: 'no-repeat',
        imageRendering: 'pixelated',
      }
    : { backgroundColor: 'rgba(0, 0, 0, 0.7)' }

  const contentClass = anchorRef
    ? `absolute ${width} bg-synt_tech_card border border-synt_tech_border rounded-xl shadow-2xl p-4 ${maxHeight} overflow-y-auto overflow-x-hidden min-w-0`
    : fitContent
      ? `absolute left-1/2 top-1/2 -translate-x-1/2 -translate-y-1/2 w-max min-w-[32rem] max-w-[95vw] bg-synt_tech_card border border-synt_tech_border rounded-xl shadow-2xl p-4 ${maxHeight} overflow-y-auto overflow-x-hidden`
      : `absolute left-1/2 top-1/2 -translate-x-1/2 -translate-y-1/2 ${maxWidth} w-full mx-4 bg-synt_tech_card border border-synt_tech_border rounded-xl shadow-2xl p-4 ${maxHeight} overflow-y-auto overflow-x-hidden min-w-0`

  const contentStyle = anchorPos
    ? { left: `${anchorPos.left}px`, top: `${anchorPos.top}px` }
    : {}

  return createPortal(
    <div className="fixed inset-0 z-[100]" style={overlayStyle} onClick={onClose}>
      <div
        className={contentClass}
        style={contentStyle}
        onClick={(e) => e.stopPropagation()}
        ref={contentRef || popupRef}
      >
        {(title || showCloseButton) && (
          <div className="flex justify-between items-center mb-3">
            {title && <h3 className="text-sm font-bold text-synt_tech_gold">{title}</h3>}
            {showCloseButton && (
              <button
                onClick={onClose}
                className="text-synt_tech_purple hover:text-synt_tech_gold text-lg leading-none ml-auto"
              >
                ×
              </button>
            )}
          </div>
        )}
        {children}
      </div>
    </div>,
    document.body
  )
}
