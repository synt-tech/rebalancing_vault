import { useEffect, useId, useMemo, useRef, useState } from 'react'
import { Minus, Plus, RotateCcw } from 'lucide-react'
import { tickToPrice, formatPriceValue } from '../utils/format'

const GOLD = '#C1A100'
const OTHERS = '#a020f0' // everyone else's liquidity — classic purple

/**
 * Concentrated-liquidity distribution histogram (aqua.network style).
 *
 * Two view modes (togglable when showModeToggle is set):
 *  - "all": full pool curve walked from the factory tick_map. Per tick bin,
 *    the liquidity covered by the viewer's own positions is stacked GOLD at
 *    the bottom of the bar, the rest of the pool's liquidity stays purple on
 *    top. Own positions are overlaid as draggable outlined handles.
 *  - "own": bars only for the given positions' tick ranges (also draggable).
 *
 * highlightIdx (index into `positions`) emphasizes one position: its bins
 * stay bright gold while every other position's bins darken.
 *
 * draggedIdx + draggedRange preview a position being drag-moved; its bar is
 * rendered at the preview range instead of its stored range.
 *
 * When onPositionDragStart(idx, clientX, pricePerPx) is provided, own
 * position bars can be click-and-dragged sideways to move their range — the
 * drag maps cursor movement linearly onto the price-linear axis.
 *
 * Navigation:
 *  - plain mouse wheel zooms the price (horizontal) window in/out
 *  - ctrl+wheel stretches/compresses the vertical (liquidity) scale
 *  - click-and-drag pans the price window horizontally; the vertical
 *    component of the same drag stretches/compresses the liquidity scale
 *    (drag up = stretch, drag down = compress)
 *  - −/+ buttons zoom horizontally, ⟳ resets
 *
 * A plain click (no drag) inside the figure that is NOT on one of the own
 * positions calls onDeselect(), so a selected position can be cleared.
 */
export default function LiquidityDistribution({
  title,
  tickMap = {},
  currentTick = 0,
  activeLiquidity = 0,
  positions = [],
  mode = 'pool',
  showModeToggle = false,
  highlightIdx = null,
  onPositionDragStart = null,
  onPositionHover = null,
  onDeselect = null,
  draggedIdx = null,
  draggedRange = null,
  token0Code = 'token0',
  token1Code = 'token1',
  tickSpacing = 1,
  height = 180,
  initialWindowPct = 0.15,
  // Raw→human price factor: tick prices are raw-unit ratios, so the whole
  // chart works in human units when multiplied by 10^(decimals0-decimals1).
  priceScale = 1,
}) {
  const t2p = (tick) => tickToPrice(tick) * priceScale
  const [view, setView] = useState(mode === 'user' ? 'user' : 'all')
  const [windowPct, setWindowPct] = useState(initialWindowPct)
  const [panOffset, setPanOffset] = useState(0) // price offset of the window center
  const [vScale, setVScale] = useState(1) // vertical stretch of the bars
  const [svgW, setSvgW] = useState(560) // live width of the svg box (1:1 px mapping)
  const svgWrapRef = useRef(null)
  // live view params for event handlers that attach once (wheel)
  const viewRef = useRef({ pct: initialWindowPct, pan: 0, v: 1 })
  viewRef.current = { pct: windowPct, pan: panOffset, v: vScale }
  const dragRef = useRef(null)
  const clipId = useId()

  // Track the svg box width so the viewBox matches CSS pixels 1:1 (no
  // preserveAspectRatio letterboxing that would shrink/center the figure)
  useEffect(() => {
    const el = svgWrapRef.current
    if (!el || typeof ResizeObserver === 'undefined') return undefined
    const ro = new ResizeObserver((entries) => {
      for (const en of entries) {
        const w = Math.round(en.contentRect.width)
        if (w > 100) setSvgW(w)
      }
    })
    ro.observe(el)
    return () => ro.disconnect()
  }, [])

  // Mouse wheel = zoom the price window (never page scroll).
  // Ctrl+wheel instead stretches/compresses the vertical (liquidity) scale.
  useEffect(() => {
    const el = svgWrapRef.current
    if (!el) return undefined
    const onWheel = (e) => {
      e.preventDefault()
      // ctrl+wheel: vertical (v-axis) stretch/compress
      if (e.ctrlKey || e.metaKey) {
        const v2 = Math.min(20, Math.max(0.1, viewRef.current.v * (e.deltaY > 0 ? 0.85 : 1.176)))
        viewRef.current = { ...viewRef.current, v: v2 }
        setVScale(v2)
        return
      }
      const factor = e.deltaY > 0 ? 1.25 : 0.8
      // Zoom anchored on the CURRENT PRICE: the pan offset scales with the
      // span so the gold line keeps its screen position while zooming,
      // even after the view was shifted by click-and-drag.
      const { pct, pan, v } = viewRef.current
      const cur = t2p(currentTick)
      const pct2 = Math.min(3, Math.max(0.005, pct * factor))
      const panLim = cur * pct2 / 2
      const pan2 = Math.max(-panLim, Math.min(panLim, pan * factor))
      viewRef.current = { pct: pct2, pan: pan2, v }
      setWindowPct(pct2)
      setPanOffset(pan2)
    }
    el.addEventListener('wheel', onWheel, { passive: false })
    return () => el.removeEventListener('wheel', onWheel)
  }, [])

  // Fit the price window around the own positions' ranges (+ current price)
  // so single narrow positions are well visible in the own-positions view.
  const fitToOwn = () => {
    const cur = t2p(currentTick)
    const prices = positions.flatMap(p => [
      t2p(Math.min(p.tick_lower, p.tick_upper)),
      t2p(Math.max(p.tick_lower, p.tick_upper)),
    ])
    if (prices.length === 0) return
    const lo = Math.min(...prices, cur)
    const hi = Math.max(...prices, cur)
    const span = Math.max((hi - lo) * 2.5, cur * 0.002)
    const pct = Math.min(3, Math.max(0.005, span / (2 * cur)))
    viewRef.current = { pct, pan: lo + (hi - lo) / 2 - cur, v: viewRef.current.v }
    setWindowPct(pct)
    setPanOffset(lo + (hi - lo) / 2 - cur)
  }

  const zoom = (factor) => {
    const pct2 = Math.min(3, Math.max(0.005, windowPct * factor))
    const panLim = curPrice * pct2 / 2
    setPanOffset(Math.max(-panLim, Math.min(panLim, panOffset * factor)))
    setWindowPct(pct2)
  }

  // Click-and-drag: horizontal movement pans the price window (content
  // follows the cursor); vertical movement stretches/compresses the
  // liquidity (v-axis) scale — drag up = stretch, drag down = compress.
  // A click without movement on empty space (not an own position) clears the
  // selection via onDeselect; allowDeselect=false suppresses that (used when
  // the press landed on one of our own positions).
  const onDragStart = (e, { allowDeselect = true } = {}) => {
    if (e.button !== 0) return
    e.preventDefault()
    const startX = e.clientX
    const startY = e.clientY
    const startPan = panOffset
    const startV = viewRef.current.v
    const rectW = svgWrapRef.current ? svgWrapRef.current.getBoundingClientRect().width : W
    const cur = t2p(currentTick)
    const span = cur * 2 * windowPct
    let moved = false
    // 1:1 cursor panning: the drag distance maps directly onto the visible
    // price window (no zoom change, no per-drag clamping)
    const onMove = (ev) => {
      const dx = ev.clientX - startX
      const dy = ev.clientY - startY
      if (Math.abs(dx) > 3 || Math.abs(dy) > 3) moved = true
      const next = startPan - (dx * (W / rectW) / (W - padL - padR)) * span
      setPanOffset(next)
      if (dy !== 0) {
        // exponential mapping keeps consecutive drags reversible
        setVScale(Math.min(20, Math.max(0.1, startV * Math.exp(-dy / 180))))
      }
    }
    const onUp = () => {
      window.removeEventListener('mousemove', onMove)
      window.removeEventListener('mouseup', onUp)
      dragRef.current = null
      // plain click on non-position area → clear the selected position
      if (!moved && allowDeselect && onDeselect) onDeselect()
    }
    window.addEventListener('mousemove', onMove)
    window.addEventListener('mouseup', onUp)
  }

  // Start dragging an own position's range (price units per screen pixel
  // derived from the visible price window — the axis is linear in price, so
  // this scale lets the segment track the cursor 1:1)
  const startOwnDrag = (e, posIdx) => {
    if (!onPositionDragStart || e.button !== 0) return
    e.preventDefault()
    e.stopPropagation()
    const rect = svgWrapRef.current ? svgWrapRef.current.getBoundingClientRect() : { width: W }
    const pricePerPx = ((maxPrice - minPrice) / plotW) * (W / rect.width)
    onPositionDragStart(posIdx, e.clientX, pricePerPx)
  }
  const curPrice = t2p(currentTick)
  // Window center = current price + pan offset (drag shifts the view in price)
  const span = curPrice * 2 * windowPct
  const center = curPrice + panOffset
  const minPrice = center - span / 2
  const maxPrice = center + span / 2

  // One bar per own position range (height ∝ position liquidity); the bar of
  // the position being drag-moved is rendered at the preview range instead.
  // Bars are layered by FIGURE AREA, largest first — the smallest overlapping
  // position ends up on top so hover/drag picks the most specific one.
  const ownBars = useMemo(() => {
    return positions
      .map((p, posIdx) => {
        let lo = t2p(Math.min(p.tick_lower, p.tick_upper))
        let hi = t2p(Math.max(p.tick_lower, p.tick_upper))
        if (posIdx === draggedIdx && draggedRange) {
          lo = draggedRange.lo
          hi = draggedRange.hi
        }
        return {
          x0: Math.max(lo, minPrice),
          x1: Math.min(hi, maxPrice),
          liq: Number(p.liquidity),
          inRange: p.tick_lower <= currentTick && currentTick <= p.tick_upper,
          posIdx,
        }
      })
      .filter(b => b.x1 > b.x0 && Number(b.liq) > 0)
      .map(b => ({ ...b, area: (b.x1 - b.x0) * b.liq }))
      .sort((a, b) => b.area - a.area)
      .map((b, i, arr) => ({ ...b, opacity: Math.max(0.35, 0.9 - i * (0.55 / Math.max(1, arr.length - 1))) }))
  }, [positions, draggedIdx, draggedRange, minPrice, maxPrice, currentTick])

  const poolBars = useMemo(() => {
    if (view !== 'all') return []
    // Walk liquidity_net over sorted ticks (Uniswap V3 semantics).
    const ticks = Object.entries(tickMap)
      .map(([t, net]) => ({ tick: Number(t), net: Number(net) }))
      .filter(t => Number.isFinite(t.tick) && Number.isFinite(t.net))
      .sort((a, b) => a.tick - b.tick)
    if (ticks.length === 0) return []

    const bins = []
    let acc = 0
    for (let i = 0; i < ticks.length - 1; i++) {
      acc += ticks[i].net
      const lo = t2p(ticks[i].tick)
      const hi = t2p(ticks[i + 1].tick)
      if (hi < minPrice || lo > maxPrice) continue
      if (acc <= 0) continue
      // How much of this bin's liquidity belongs to the viewer's own
      // positions: a position covers the whole bin when its range spans it
      // (position bounds are ticks in the map, so bins never straddle them).
      // hlLiq = the part owned by the currently highlighted position, so the
      // stacked rendering can keep it bright and fade the rest.
      const t0 = ticks[i].tick
      const t1 = ticks[i + 1].tick
      let ownLiq = 0
      let hlLiq = 0
      positions.forEach((p, pi) => {
        if (p.tick_lower <= t0 && p.tick_upper >= t1) {
          const l = Number(p.liquidity)
          ownLiq += l
          if (pi === highlightIdx) hlLiq += l
        }
      })
      bins.push({ x0: Math.max(lo, minPrice), x1: Math.min(hi, maxPrice), liq: acc, ownLiq: Math.min(ownLiq, acc), hlLiq: Math.min(hlLiq, acc) })
    }
    return bins
  }, [view, tickMap, positions, highlightIdx, minPrice, maxPrice])

  const bars = view === 'user' ? ownBars : poolBars

  // Layout — W follows the rendered box width (1:1 CSS px), and the pads are
  // tuned to align the baseline and axis labels with the PoolCharts figures
  // (pane 144px tall, 26px time axis with 10px labels).
  const W = svgW
  const H = height
  const padL = 8
  const padR = 8
  const padT = 18
  const padB = 23
  const plotW = W - padL - padR
  const plotH = H - padT - padB

  const maxLiq = Math.max(1, ...bars.map(b => b.liq), ...(view === 'all' ? ownBars.map(b => b.liq) : []))
  const x = (price) => padL + ((price - minPrice) / (maxPrice - minPrice)) * plotW
  const curX = x(Math.min(Math.max(curPrice, minPrice), maxPrice))

  // Adaptive x-axis ticks: 1-2-5 progression over the visible price window —
  // zooming/panning rescales the step so finer labels appear as you zoom in.
  // The step is never finer than one pool tick-spacing, and the label
  // precision adapts to the step (no excess trailing zeros).
  const axisTicks = []
  let axisDigits = 4
  {
    const span = maxPrice - minPrice
    if (span > 0) {
      const rawStep = span / 4
      const mag = Math.pow(10, Math.floor(Math.log10(rawStep)))
      const norm = rawStep / mag
      let step = (norm >= 5 ? 5 : norm >= 2 ? 2 : 1) * mag
      const tickStep = curPrice * 0.0001 * (tickSpacing || 1)
      if (tickStep > step) step = tickStep
      axisDigits = Math.max(0, Math.ceil(-Math.log10(step)))
      for (let p = Math.ceil(minPrice / step) * step; p <= maxPrice; p += step) {
        axisTicks.push(p)
      }
    }
  }

  const axisLabel = (p) => {
    if (!isFinite(p)) return ''
    const s = p.toFixed(axisDigits)
    return s.includes('.') ? s.replace(/0+$/, '').replace(/\.$/, '') : s
  }

  // Hit-testing for own-position handles: all handles overlapping the cursor
  // in BOTH x (price) and y (bar height) are considered, and the one with the
  // SMALLEST rendered (pixel) area wins, so nested smaller positions are easy
  // to target regardless of render order.
  const handleAt = (clientX, clientY, svgEl) => {
    if (!svgEl || ownBars.length === 0) return null
    const rect = svgEl.getBoundingClientRect()
    const price = minPrice + ((clientX - rect.left - padL) / plotW) * (maxPrice - minPrice)
    let best = null
    let bestArea = Infinity
    for (const b of ownBars) {
      if (price < b.x0 || price > b.x1) continue
      // y-overlap: the cursor must sit within the bar's drawn height
      // (mirrors the vertical stretch applied to the rendered bars)
      const pxH = Math.max(2, (b.liq / maxLiq) * (plotH - 6) * vScale)
      const barTop = padT + plotH - pxH
      const svgY = clientY - rect.top
      if (svgY < barTop || svgY > padT + plotH) continue
      const pxW = Math.max(1, x(b.x1) - x(b.x0))
      const a = pxW * pxH
      if (a < bestArea) {
        bestArea = a
        best = b
      }
    }
    return best
  }

  const onWrapperMouseMove = (e) => {
    if (!onPositionHover) return
    const best = handleAt(e.clientX, e.clientY, e.currentTarget)
    onPositionHover(best ? best.posIdx : null)
  }

  const onWrapperMouseDown = (e) => {
    if (e.button !== 0) return
    const best = handleAt(e.clientX, e.clientY, e.currentTarget)
    if (best && onPositionDragStart) {
      startOwnDrag(e, best.posIdx)
    } else {
      // pressing on an own position must NEVER clear the selection, even when
      // position dragging is disabled (view-only / no handler)
      onDragStart(e, { allowDeselect: !best })
    }
  }

  const draggable = !!onPositionDragStart
  const ownBarStyle = (b) => {
    const isDragged = b.posIdx === draggedIdx
    const isHighlighted = b.posIdx === highlightIdx
    const dimmed = highlightIdx != null && !isHighlighted && !isDragged
    if (view === 'all') {
      // In the all-liquidity view the stacked bars already show the own
      // share per bin — the per-position overlay is a draggable handle
      // outlining the exact range. The hovered/selected position gets a
      // purple outline while every other position fades. Faded handles stay
      // interactive on purpose: they're layered smallest-on-top, so moving
      // the cursor across overlapping ranges re-targets the highlight to
      // whichever handle is under the cursor (e.g. a nested smaller one).
      return {
        fill: isHighlighted ? 'rgba(160, 32, 240, 0.15)' : 'transparent',
        opacity: dimmed ? 0.25 : 1,
        stroke: isDragged || isHighlighted ? '#a020f0' : GOLD,
        strokeWidth: isDragged || isHighlighted ? 3 : 1.5,
        cursor: draggable ? 'ew-resize' : undefined,
      }
    }
    return {
      fill: GOLD,
      opacity: dimmed ? 0.25 : (isDragged ? 1 : (b.opacity ?? 0.6)),
      stroke: isHighlighted ? '#a020f0' : 'none',
      strokeWidth: 2,
      cursor: draggable ? 'ew-resize' : undefined,
    }
  }

  return (
    <div className="rounded-lg border border-synt_tech_border bg-black/30 p-3 flex flex-col">
      <div className="flex items-center justify-between mb-1">
        <span className="text-xs text-synt_tech_purple/70">{title}</span>
        <div className="flex items-center gap-1">
          {showModeToggle && (
            <button
              type="button"
              className="inline-flex items-center h-5 px-2 text-xs leading-none text-synt_tech_purple hover:text-synt_tech_gold border border-synt_tech_border rounded"
              title={view === 'all' ? 'showing all pool liquidity — click to show only your positions' : 'showing only your positions — click to show all pool liquidity'}
              onClick={() => {
                const next = view === 'all' ? 'user' : 'all'
                setView(next)
                if (next === 'user') fitToOwn()
                else { setWindowPct(initialWindowPct); setPanOffset(0) }
              }}
            >
              {view === 'all' ? 'all liquidity' : 'own positions'}
            </button>
          )}
          <button
            type="button"
            className="inline-flex items-center justify-center w-5 h-5 text-synt_tech_purple hover:text-synt_tech_gold border border-synt_tech_border rounded"
            title="zoom out"
            onClick={() => zoom(1.25)}
          >
            <Minus className="w-3 h-3" />
          </button>
          <button
            type="button"
            className="inline-flex items-center justify-center w-5 h-5 text-synt_tech_purple hover:text-synt_tech_gold border border-synt_tech_border rounded"
            title="zoom in"
            onClick={() => zoom(0.8)}
          >
            <Plus className="w-3 h-3" />
          </button>
          <button
            type="button"
            className="inline-flex items-center justify-center w-5 h-5 text-synt_tech_purple hover:text-synt_tech_gold border border-synt_tech_border rounded"
            title="reset zoom"
            onClick={() => {
              setVScale(1)
              if (view === 'user') fitToOwn()
              else { setWindowPct(initialWindowPct); setPanOffset(0) }
            }}
          >
            <RotateCcw className="w-3 h-3" />
          </button>
        </div>
      </div>
      <div className="relative cursor-grab active:cursor-grabbing flex-1" data-keep-selection ref={svgWrapRef} onMouseDown={onWrapperMouseDown} onMouseMove={onWrapperMouseMove} onMouseLeave={() => { if (onPositionHover) onPositionHover(null) }}>
        <svg viewBox={`0 0 ${W} ${H}`} className="w-full block" style={{ height }}>
          {/* bars are clipped to the plot area so a vertical stretch doesn't
              spill over the axis labels / title */}
          <defs>
            <clipPath id={clipId}>
              <rect x={padL} y={padT} width={plotW} height={plotH} />
            </clipPath>
          </defs>
          {/* empty state for user mode with no positions */}
          {view === 'user' && positions.length === 0 && (
            <text x={W / 2} y={padT + plotH / 2} textAnchor="middle" fill="#a020f088" fontSize={11} fontFamily="monospace">
              no positions found in this pool
            </text>
          )}
          <g clipPath={`url(#${clipId})`}>
          {/* pool curve bars (all-liquidity view): per bin, stacked segments —
              bright GOLD for the highlighted position, dim gold for other own
              positions, purple for the rest of the pool. With no highlight,
              all own liquidity renders as one bright gold segment. */}
          {view === 'all' && bars.map((b, i) => {
            const x0 = x(b.x0)
            const x1 = x(b.x1)
            const w = Math.max(1, x1 - x0)
            const h = Math.max(2, (b.liq / maxLiq) * (plotH - 6) * vScale)
            const ownLiq = Math.min(b.ownLiq || 0, b.liq)
            const hlLiq = Math.min(b.hlLiq || 0, ownLiq)
            const hOwn = ownLiq > 0 ? Math.max(1, (ownLiq / maxLiq) * (plotH - 6) * vScale) : 0
            const hHl = hlLiq > 0 ? (hlLiq / maxLiq) * (plotH - 6) * vScale : 0
            const hOtherOwn = Math.max(0, hOwn - hHl)
            const hPurple = Math.max(0, h - hOwn)
            const hlActive = highlightIdx != null
            const base = padT + plotH
            return (
              <g key={`pool-${i}`}>
                <rect x={x0} y={base - h} width={w} height={hPurple} fill={OTHERS} opacity={hlActive ? 0.35 : 0.7} rx={1} />
                {hOtherOwn > 0 && (
                  <rect x={x0} y={base - hHl - hOtherOwn} width={w} height={hOtherOwn} fill={GOLD} opacity={hlActive ? 0.25 : 0.95} rx={1} />
                )}
                {hHl > 0 && (
                  <rect x={x0} y={base - hHl} width={w} height={hHl} fill={GOLD} opacity={1} rx={1} />
                )}
              </g>
            )
          })}
          {/* own position bars (main in own view, overlay in all view) — draggable */}
          {ownBars.map((b, i) => {
            const x0 = x(b.x0)
            const x1 = x(b.x1)
            const h = Math.max(2, (b.liq / maxLiq) * (plotH - 6) * vScale)
            const s = ownBarStyle(b)
            return (
              <rect
                key={`own-${i}`}
                x={x0}
                y={padT + plotH - h}
                width={Math.max(1, x1 - x0)}
                height={h}
                fill={s.fill}
                opacity={s.opacity}
                stroke={s.stroke}
                strokeWidth={s.strokeWidth}
                rx={1}
                style={{ pointerEvents: 'none' }}
              />
            )
          })}
          </g>
          {/* baseline */}
          <line x1={padL} y1={padT + plotH} x2={W - padR} y2={padT + plotH} stroke="#4a2040" strokeWidth={1} />
          {/* current price line: gold dashes with a purple casing so the
              dashed line stands out against both gold (own) and purple
              (others) bars it crosses */}
          <line x1={curX} y1={padT - 4} x2={curX} y2={padT + plotH} stroke="#a020f0" strokeWidth={3.5} strokeDasharray="4 3" strokeLinecap="round" />
          <line x1={curX} y1={padT - 4} x2={curX} y2={padT + plotH} stroke="#C1A100" strokeWidth={1.5} strokeDasharray="4 3" strokeLinecap="round" />
          {/* current price label */}
          <text x={Math.min(Math.max(curX, padL + 60), W - padR - 60)} y={padT - 8} textAnchor="middle" fill="#C1A100" fontSize={10} fontFamily="monospace">
            current price ≈ {formatPriceValue(curPrice)} {token1Code}/{token0Code}
          </text>
          {/* adaptive x-axis ticks */}
          {axisTicks.map(p => (
            <g key={p}>
              <line x1={x(p)} y1={padT + plotH} x2={x(p)} y2={padT + plotH + 4} stroke="#a020f0" />
              <text x={x(p)} y={H - 6} textAnchor="middle" fill="#a020f0" fontSize={10} fontFamily="monospace">{axisLabel(p)}</text>
            </g>
          ))}
        </svg>
      </div>
    </div>
  )
}
