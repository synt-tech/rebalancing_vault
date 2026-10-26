import { useEffect, useLayoutEffect, useRef, useState } from 'react'
import { ChevronDown } from 'lucide-react'

// useLayoutEffect warns under renderToString (SSR tests) — fall back there.
const useIsomorphicLayoutEffect =
  typeof window !== 'undefined' ? useLayoutEffect : useEffect

// Shared compact controls — ONE box style for the whole site so the 8-bit
// Operator pixel font is centred identically everywhere. The font's ink sits
// high in its line box (ascent ≈ 8px vs descent ≈ 1px at 13px), so every box
// uses leading-none plus a bottom-biased padding — the same direction as the
// legacy global `select { padding-bottom: 7px }` hack. UiSelect replaces the
// native <select>, whose option-text clipping is OS-widget dependent (GTK
// clipped glyph tops at py-1 and bottoms at py-1.5).
const UI_BOX_CLS =
  'text-xs leading-none px-2 pt-[5px] pb-[7px] border border-synt_tech_border rounded bg-synt_tech_card text-synt_tech_gold'

// Compact text input — same box chrome as UiSelect; extra classes merge in
// (e.g. pl-7 for an icon, w-44 for width).
export function UiInput({ className = '', ...props }) {
  return (
    <input
      {...props}
      className={`${UI_BOX_CLS} placeholder:text-synt_tech_gold/60 ${className}`}
    />
  )
}

// Compact dropdown: a styled button + absolutely-positioned option list.
// options: [{ value, label, logo?, disabled? }] — value is compared by ===
// (numbers or strings). Disabled options render dimmed and cannot be picked
// via mouse or keyboard.
export function UiSelect({ value, onChange, options, ariaLabel, title, className = '' }) {
  const [open, setOpen] = useState(false)
  const [hi, setHi] = useState(-1) // keyboard-highlighted option index
  const ref = useRef(null)
  const btnRef = useRef(null)
  const measureRef = useRef(null)
  const listRef = useRef(null)
  const [autoWidth, setAutoWidth] = useState(null)
  const selected = options.find((o) => o.value === value) ?? options[0]

  useEffect(() => {
    const handler = (e) => {
      if (ref.current && !ref.current.contains(e.target)) setOpen(false)
    }
    document.addEventListener('mousedown', handler)
    return () => document.removeEventListener('mousedown', handler)
  }, [])

  // Scroll the highlighted/selected option into view when the list opens or
  // the keyboard highlight moves — long lists start at the top otherwise.
  useIsomorphicLayoutEffect(() => {
    if (!open || !listRef.current) return
    const el = listRef.current.querySelector('[data-active="true"]')
    el?.scrollIntoView({ block: 'nearest' })
  }, [open, hi])

  // Fixed min-width = widest option so opening never reflows the row.
  useIsomorphicLayoutEffect(() => {
    if (measureRef.current) setAutoWidth(measureRef.current.offsetWidth)
  }, [options])

  const firstEnabledFrom = (start) => {
    const n = options.length
    if (start >= 0 && !options[start]?.disabled) return start
    for (let i = 0; i < n; i++) {
      const j = (Math.max(start, 0) + i) % n
      if (!options[j].disabled) return j
    }
    return -1
  }

  const moveHi = (dir) => {
    const n = options.length
    let j = hi
    for (let i = 0; i < n; i++) {
      j = (j + dir + n) % n
      if (!options[j].disabled) { setHi(j); return }
    }
  }

  const openList = () => {
    setHi(firstEnabledFrom(options.findIndex((o) => o.value === value)))
    setOpen(true)
  }

  const commit = (opt) => {
    if (!opt || opt.disabled) return
    onChange(opt.value)
    setOpen(false)
    // A clicked option unmounts with the list — return focus to the trigger
    // so keyboard users don't lose their place.
    btnRef.current?.focus()
  }

  const onKeyDown = (e) => {
    if (e.key === 'Escape') { setOpen(false); btnRef.current?.focus() }
    else if (e.key === 'Enter' || e.key === ' ') {
      e.preventDefault()
      if (open) commit(options[hi])
      else openList()
    } else if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
      e.preventDefault()
      if (!open) openList()
      else moveHi(e.key === 'ArrowDown' ? 1 : -1)
    }
  }

  return (
    <div className={`relative ${className}`} ref={ref}>
      {/* Hidden column to measure the widest option */}
      <div ref={measureRef} aria-hidden="true" className="invisible fixed flex flex-col" style={{ left: -9999, top: 0 }}>
        {options.map((opt) => (
          <div key={String(opt.value)} className="text-xs leading-none flex items-center gap-1.5 px-2 py-1 whitespace-nowrap">
            {opt.logo && <span style={{ width: 16, height: 16, display: 'inline-block' }} />}
            <span>{opt.label}</span>
          </div>
        ))}
      </div>
      <button
        type="button"
        ref={btnRef}
        aria-label={ariaLabel}
        title={title}
        aria-haspopup="listbox"
        aria-expanded={open}
        onClick={() => (open ? setOpen(false) : openList())}
        onKeyDown={onKeyDown}
        className={`${UI_BOX_CLS} flex items-center gap-1.5 whitespace-nowrap hover:border-synt_tech_gold/50`}
        style={autoWidth ? { minWidth: autoWidth } : undefined}
      >
        {selected?.logo && (
          <img src={selected.logo} alt="" className="w-4 h-4 rounded-full" onError={(e) => { e.target.style.display = 'none' }} />
        )}
        <span>{selected?.label}</span>
        <ChevronDown className="w-3 h-3 opacity-60 ml-auto" />
      </button>
      {open && (
        <div role="listbox" ref={listRef} className="absolute z-50 mt-1 w-full max-h-64 overflow-y-auto bg-synt_tech_card border border-synt_tech_border rounded shadow-xl">
          {options.map((opt, i) => (
            <button
              key={String(opt.value)}
              type="button"
              role="option"
              aria-selected={opt.value === value}
              data-active={i === hi}
              disabled={opt.disabled}
              onClick={() => commit(opt)}
              onMouseEnter={() => !opt.disabled && setHi(i)}
              className={`w-full text-left text-xs leading-none flex items-center gap-1.5 px-2 pt-1.5 pb-2 whitespace-nowrap ${
                opt.disabled
                  ? 'opacity-40 cursor-not-allowed'
                  : 'hover:bg-synt_tech_purple/10'
              } ${i === hi ? 'bg-synt_tech_purple/10' : ''} ${opt.value === value ? 'text-synt_tech_gold' : 'text-synt_tech_purple'}`}
            >
              {opt.logo && (
                <img src={opt.logo} alt="" className="w-4 h-4 rounded-full" onError={(e) => { e.target.style.display = 'none' }} />
              )}
              <span>{opt.label}</span>
            </button>
          ))}
        </div>
      )}
    </div>
  )
}
