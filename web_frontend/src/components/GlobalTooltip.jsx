import { useEffect, useRef, useState } from 'react'
import { createPortal } from 'react-dom'

// Global themed tooltip — THE tooltip for the whole app.
//
// Native `title` tooltips render in the browser's own white/chrome styling,
// which clashes with the theme and cannot be restyled. Instead of requiring
// every call site to use a wrapper component (and silently falling back to
// native when one forgets), this singleton delegates on `mouseover`/`focusin`:
// any element with a `title` (or `data-tip`) attribute gets the themed tooltip
// here — the `title` is moved to `data-tip` on first hover so the native one
// never appears. Existing and future `title="..."` markup keeps working and
// automatically gets the themed treatment.
//
// Styling follows the established custom tooltip (MarketHistory): gold text
// on the darker card background with a faint gold border.

const SHOW_DELAY_MS = 350
const GAP_PX = 6
const EDGE_PX = 8

export default function GlobalTooltip() {
  const [tip, setTip] = useState(null) // { text, x, y, placeAbove }
  const timer = useRef(null)
  const target = useRef(null)
  const tipRef = useRef(null)
  const watcher = useRef(null)

  useEffect(() => {
    // Watch the hovered element: a React re-render can stamp a fresh `title`
    // on it (dynamic tooltip text — e.g. the refresh button listing active
    // fetches). Relocating it immediately keeps the native white tooltip
    // suppressed and the displayed text current.
    const observeTarget = (el) => {
      watcher.current?.disconnect()
      watcher.current = new MutationObserver(() => {
        if (el.hasAttribute('title')) {
          el.setAttribute('data-tip', el.getAttribute('title'))
          el.removeAttribute('title')
        }
        const text = el.getAttribute('data-tip')
        setTip(prev => (prev && target.current === el && prev.text !== text ? { ...prev, text } : prev))
      })
      watcher.current.observe(el, { attributes: true, attributeFilter: ['title', 'data-tip'] })
    }
    const cancel = () => {
      if (timer.current) { clearTimeout(timer.current); timer.current = null }
      watcher.current?.disconnect()
      watcher.current = null
      target.current = null
      setTip(null)
    }
    const show = (el, immediate = false) => {
      const r = el.getBoundingClientRect()
      const placeBelow = () => setTip({ text: el.getAttribute('data-tip'), x: r.left + r.width / 2, y: r.bottom + GAP_PX, placeAbove: false, measured: false })
      if (immediate) placeBelow()
      else timer.current = setTimeout(placeBelow, SHOW_DELAY_MS)
    }
    const onOver = (e) => {
      const el = e.target instanceof Element ? e.target.closest('[title],[data-tip]') : null
      if (!el) { if (target.current) cancel(); return }
      // Relocate the attribute so the browser's native tooltip never shows.
      const text = el.getAttribute('title') || el.getAttribute('data-tip')
      if (!text) return
      if (el.hasAttribute('title')) {
        el.setAttribute('data-tip', text)
        el.removeAttribute('title')
      }
      if (target.current === el) return
      cancel()
      target.current = el
      observeTarget(el)
      show(el)
    }
    const onOut = (e) => {
      if (target.current && !(e.relatedTarget instanceof Node && target.current.contains(e.relatedTarget))) cancel()
    }
    const onFocus = (e) => {
      if (!(e.target instanceof Element)) return
      const el = e.target.closest('[title],[data-tip]')
      if (!el) return
      const text = el.getAttribute('title') || el.getAttribute('data-tip')
      if (!text) return
      if (el.hasAttribute('title')) {
        el.setAttribute('data-tip', text)
        el.removeAttribute('title')
      }
      cancel()
      target.current = el
      observeTarget(el)
      show(el, true)
    }
    const onBlur = (e) => {
      if (target.current === e.target) cancel()
    }
    document.addEventListener('mouseover', onOver)
    document.addEventListener('mouseout', onOut)
    document.addEventListener('focusin', onFocus)
    document.addEventListener('focusout', onBlur)
    document.addEventListener('mousedown', cancel, true)
    document.addEventListener('scroll', cancel, true)
    document.addEventListener('keydown', cancel)
    window.addEventListener('blur', cancel)
    return () => {
      document.removeEventListener('mouseover', onOver)
      document.removeEventListener('mouseout', onOut)
      document.removeEventListener('focusin', onFocus)
      document.removeEventListener('focusout', onBlur)
      document.removeEventListener('mousedown', cancel, true)
      document.removeEventListener('scroll', cancel, true)
      document.removeEventListener('keydown', cancel)
      window.removeEventListener('blur', cancel)
      watcher.current?.disconnect()
      if (timer.current) clearTimeout(timer.current)
    }
  }, [])

  // Second pass once mounted: the tooltip's real height decides whether it
  // sits below the target or flips above it, and clamps it inside the viewport.
  useEffect(() => {
    if (!tip || tip.measured || !tipRef.current) return
    const el = tipRef.current
    const h = el.offsetHeight
    const w = el.offsetWidth
    const anchor = target.current && document.contains(target.current)
      ? target.current.getBoundingClientRect() : null
    if (!anchor) { setTip(null); return }
    const fitsBelow = anchor.bottom + GAP_PX + h <= window.innerHeight - EDGE_PX
    const placeAbove = !fitsBelow
    const x = Math.min(Math.max(anchor.left + anchor.width / 2, w / 2 + EDGE_PX), window.innerWidth - w / 2 - EDGE_PX)
    setTip({
      text: tip.text, x,
      y: placeAbove ? anchor.top - GAP_PX : anchor.bottom + GAP_PX,
      placeAbove, measured: true,
    })
  }, [tip])

  if (!tip) return null
  return createPortal(
    <div
      ref={tipRef}
      role="tooltip"
      className="fixed z-[100] max-w-[320px] px-2 py-1 text-xs text-synt_tech_gold bg-synt_tech_darker border border-synt_tech_gold/40 rounded whitespace-pre-line pointer-events-none"
      style={{
        left: tip.x,
        top: tip.y,
        transform: tip.placeAbove ? 'translate(-50%, -100%)' : 'translate(-50%, 0)',
        visibility: tip.measured ? 'visible' : 'hidden',
      }}
    >
      {tip.text}
    </div>,
    document.body
  )
}
