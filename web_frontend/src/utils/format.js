/**
 * Shared number formatting utilities.
 * Uses apostrophe (') as thousands separator for all number displays.
 */

// BigInt stroops (1e7) → decimal string; inlined to keep this module leaf-only.
const STROOPS_PER_UNIT = 10_000_000n
export function fromStroops(stroops) {
  const s = BigInt(stroops)
  const negative = s < 0n
  const abs = negative ? -s : s
  const whole = abs / STROOPS_PER_UNIT
  const frac = (abs % STROOPS_PER_UNIT).toString().padStart(7, '0').replace(/0+$/, '')
  const str = frac ? `${whole}.${frac}` : whole.toString()
  return negative ? `-${str}` : str
}

/**
 * Insert ' thousands separators into an integer-part digit string.
 * THE shared grouping step — every displayed or typed amount groups through
 * this; do NOT re-implement the regex elsewhere.
 * e.g. "19386166" -> "19'386'166"
 * @param {string|number} intPart
 * @returns {string}
 */
export function groupThousands(intPart) {
  return String(intPart).replace(/\B(?=(\d{3})+(?!\d))/g, "'")
}

/**
 * Format a number with apostrophe thousands separator.
 * e.g. 19386166.6142 -> "19'386'166.6142"
 * @param {number|string} val - The number to format
 * @param {number} [decimals=4] - Number of decimal places
 * @returns {string}
 */
export function formatNum(val, decimals = 4) {
  if (val == null || val === '') return '0'
  const n = parseFloat(val)
  if (isNaN(n)) return '0'
  if (n === 0) return '0'
  const fixed = n.toFixed(decimals)
  const [intPart, fracPart] = fixed.split('.')
  const withSep = groupThousands(intPart)
  return fracPart ? `${withSep}.${fracPart}` : withSep
}

/**
 * Format a token amount with appropriate precision.
 * Large numbers get compact suffixes (K, M, B, T).
 * Small numbers get more decimals.
 * @param {number|string} val - The amount
 * @param {number} [decimals=7] - Max decimal places (unused if compact)
 * @returns {string}
 */
export function formatAmount(val, decimals = 7) {
  if (!val) return '0'
  const n = parseFloat(val)
  if (n === 0) return '0'
  // magnitude checks run on the absolute value — negatives would otherwise
  // fall through to the tiny-value exponential branch
  const sign = n < 0 ? '-' : ''
  const a = Math.abs(n)
  if (a >= 1e12) return `${sign}${formatNum(a / 1e12, 2)}T`
  if (a >= 1e9) return `${sign}${formatNum(a / 1e9, 2)}B`
  if (a >= 1e6) return `${sign}${formatNum(a / 1e6, 2)}M`
  if (a >= 1e3) return `${sign}${formatNum(a / 1e3, 2)}K`
  if (a < 0.0001) return n.toExponential(2)
  if (a < 1) return formatNum(n, decimals)
  return formatNum(n, 4)
}

/**
 * Format a compact number (K, M, B, T suffixes).
 * @param {number|string} val
 * @returns {string}
 */
export function formatCompact(val, decimals = 7) {
  // identical semantics to formatAmount — single shared implementation
  return formatAmount(val, decimals)
}

/**
 * Format a USD value from raw stroops (1e7 scale).
 * @param {number|string} val - Value in stroops (7 decimals)
 * @returns {string}
 */
export function formatUSD(val) {
  if (!val) return '$0'
  const intStr = typeof val === 'string' ? val.split('.')[0] : String(val)
  const n = Number(fromStroops(BigInt(intStr)))
  if (n >= 1e9) return `$${formatNum(n / 1e9, 2)}B`
  if (n >= 1e6) return `$${formatNum(n / 1e6, 2)}M`
  if (n >= 1e3) return `$${formatNum(n / 1e3, 2)}K`
  return `$${formatNum(n, 2)}`
}

/**
 * Format a USD value from a human-readable number (not stroops).
 * @param {number} val
 * @returns {string}
 */
export function formatUsdValue(val) {
  if (!val || val <= 0) return '$0'
  if (val >= 1e6) return `$${formatNum(val / 1e6, 2)}M`
  if (val >= 1e3) return `$${formatNum(val / 1e3, 2)}K`
  if (val >= 1) return `$${formatNum(val, 2)}`
  return `$${formatNum(val, 4)}`
}

/**
 * Format a percentage from a decimal (0.0412 -> "4.12%").
 * @param {number|string} val
 * @returns {string}
 */
export function formatPct(val) {
  if (!val) return '0%'
  return `${formatNum(parseFloat(val) * 100, 2)}%`
}

/**
 * Format a percent value with magnitude-adaptive decimals (trailing zeros
 * trimmed). For sub-percent values a fixed "0.0%" hides the number entirely
 * — the drift-guard bands routinely run at fractions of a percent where
 * "0.0024%" carries real information.
 * e.g. 25 -> "25%", 0.24 -> "0.24%", 0.0024 -> "0.0024%"
 * @param {number|string} percentValue - the number BEFORE the % sign
 * @returns {string}
 */
export function formatPctPrecise(percentValue) {
  const n = parseFloat(percentValue)
  if (!Number.isFinite(n) || n === 0) return '0%'
  const a = Math.abs(n)
  const decimals = a >= 100 ? 0 : a >= 10 ? 1 : a >= 1 ? 2 : a >= 0.01 ? 4 : 6
  return `${parseFloat(n.toFixed(decimals))}%`
}

// ---- Device-local timestamps ---------------------------------------------
// THE site-wide convention for absolute times: every user-facing timestamp
// renders in the viewer's local timezone through these helpers — never
// toISOString/toLocale*String or hand-rolled `new Date()` formatting (those
// scatter UTC or platform-locale output). Output: 'YYYY-MM-DD HH:mm:ss' /
// 'YYYY-MM-DD'. datetime-local inputs are likewise device-local (see
// poolHistoryDateRange in utils/poolActivity.js).
const pad2 = (n) => String(n).padStart(2, '0')

function toLocalDate(value) {
  if (value == null || value === '') return null
  let ms
  if (value instanceof Date) {
    ms = value.getTime()
  } else if (typeof value === 'number' || (typeof value === 'string' && /^\d+$/.test(value))) {
    const n = Number(value)
    if (!Number.isFinite(n)) return null
    // epoch seconds (<1e12) vs epoch milliseconds
    ms = Math.abs(n) < 1e12 ? n * 1000 : n
  } else {
    ms = Date.parse(value)
  }
  const d = new Date(ms)
  return Number.isNaN(d.getTime()) ? null : d
}

/**
 * Absolute timestamp in the device's local timezone: 'YYYY-MM-DD HH:mm:ss'.
 * Accepts a Date, epoch seconds or milliseconds, a numeric epoch string, or
 * any string Date.parse understands (ISO, RFC2822, ...). Returns '' for
 * missing/invalid input.
 */
export function formatLocalDateTime(value) {
  const d = toLocalDate(value)
  if (!d) return ''
  return `${d.getFullYear()}-${pad2(d.getMonth() + 1)}-${pad2(d.getDate())} ${pad2(d.getHours())}:${pad2(d.getMinutes())}:${pad2(d.getSeconds())}`
}

/** Date-only variant of formatLocalDateTime: 'YYYY-MM-DD' (local timezone). */
export function formatLocalDate(value) {
  const d = toLocalDate(value)
  return d ? `${d.getFullYear()}-${pad2(d.getMonth() + 1)}-${pad2(d.getDate())}` : ''
}

/** Time-only variant of formatLocalDateTime: 'HH:mm:ss' (local timezone). */
export function formatLocalTime(value) {
  const d = toLocalDate(value)
  return d ? `${pad2(d.getHours())}:${pad2(d.getMinutes())}:${pad2(d.getSeconds())}` : ''
}

/**
 * Relative timestamp for order/trade lists. Accepts epoch seconds (Horizon
 * ledger/orderbook fields) or an ISO string; renders s/m/h/d.
 */
export function timeAgo(ts) {
  const ms = typeof ts === 'string' ? Date.parse(ts) : ts * 1000
  const diff = Date.now() - ms
  if (diff < 60000) return `${Math.floor(diff / 1000)}s`
  if (diff < 3600000) return `${Math.floor(diff / 60000)}m`
  if (diff < 86400000) return `${Math.floor(diff / 3600000)}h`
  return `${Math.floor(diff / 86400000)}d`
}

/**
 * Truncate a Stellar address/key for compact display: GABCDEF...WXYZ12.
 */
export function fmtAddr(addr) {
  if (!addr) return ''
  return `${addr.slice(0, 6)}...${addr.slice(-6)}`
}

/**
 * Format a fractional share (0.0025 -> "0.25%") for pool-share displays.
 * Tiny nonzero shares render "<0.01%" so they stay distinguishable from 0.
 * @param {number|null|undefined} frac
 * @returns {string|null}
 */
export function formatSharePct(frac) {
  if (frac == null) return null
  if (frac > 0 && frac < 0.0001) return '<0.01%'
  return formatPct(frac)
}

/**
 * Convert tick to price (1.0001^tick).
 * @param {number} tick
 * @returns {number}
 */
export function tickToPrice(tick) {
  return Math.pow(1.0001, tick)
}

/**
 * Convert price to (continuous) tick — inverse of tickToPrice.
 * @param {number} price
 * @returns {number}
 */
export function priceToTick(price) {
  return Math.log(price) / Math.log(1.0001)
}

/**
 * tickToPrice adjusted for the decimals gap between the two pool tokens.
 * Tick prices are in RAW units (token1_raw per token0_raw); human-unit price
 * is raw_price * 10^(decimals0 - decimals1). Equal decimals → identical to
 * tickToPrice.
 * @param {number} tick
 * @param {number} [decimals0=7]
 * @param {number} [decimals1=7]
 * @returns {number}
 */
export function tickToPriceScaled(tick, decimals0 = 7, decimals1 = 7) {
  return tickToPrice(tick) * Math.pow(10, decimals0 - decimals1)
}

/**
 * Inverse of tickToPriceScaled — converts a HUMAN-unit price (token1 per
 * token0) to the pool's raw-unit tick.
 * @param {number} price
 * @param {number} [decimals0=7]
 * @param {number} [decimals1=7]
 * @returns {number}
 */
export function priceToTickScaled(price, decimals0 = 7, decimals1 = 7) {
  return priceToTick(price / Math.pow(10, decimals0 - decimals1))
}

/**
 * Token amounts (human units) held by a concentrated-liquidity position with
 * the given raw liquidity over [loTick, hiTick] at the current tick.
 * Uniswap-V3 semantics: below the range → all token0, above → all token1,
 * inside → a mix determined by the current price.
 * @param {number|string|bigint} liquidityRaw - raw u128 liquidity
 * @param {number} loTick
 * @param {number} hiTick
 * @param {number} curTick
 * @param {number} [decimals0=7] - token0 decimals (raw amounts are divided by 10^decimals0)
 * @param {number} [decimals1=7] - token1 decimals
 * @returns {{amount0: number, amount1: number}}
 */
export function positionTokenAmounts(liquidityRaw, loTick, hiTick, curTick, decimals0 = 7, decimals1 = 7) {
  const L = Number(liquidityRaw)
  if (!isFinite(L) || L <= 0) return { amount0: 0, amount1: 0 }
  const sqrt = (t) => Math.pow(1.0001, t / 2)
  const sl = sqrt(Math.min(loTick, hiTick))
  const sh = sqrt(Math.max(loTick, hiTick))
  let a0Raw
  let a1Raw
  if (curTick <= loTick) {
    a0Raw = L * (1 / sl - 1 / sh)
    a1Raw = 0
  } else if (curTick >= hiTick) {
    a0Raw = 0
    a1Raw = L * (sh - sl)
  } else {
    const sc = sqrt(curTick)
    a0Raw = L * (1 / sc - 1 / sh)
    a1Raw = L * (sc - sl)
  }
  return { amount0: a0Raw / Math.pow(10, decimals0), amount1: a1Raw / Math.pow(10, decimals1) }
}

/**
 * Inverse of positionTokenAmounts: the raw liquidity a position would mint for
 * the given human-unit token deposits over [loTick, hiTick] at the current
 * tick. When only one token is required (price outside the range) the other
 * token's amount is ignored.
 * @returns {number} raw liquidity, 0 when the deposits can't form a position
 */
export function liquidityForTokenAmounts(amount0, amount1, loTick, hiTick, curTick, decimals0 = 7, decimals1 = 7) {
  const a0 = Number(amount0) * Math.pow(10, decimals0)
  const a1 = Number(amount1) * Math.pow(10, decimals1)
  const sqrt = (t) => Math.pow(1.0001, t / 2)
  const sl = sqrt(Math.min(loTick, hiTick))
  const sh = sqrt(Math.max(loTick, hiTick))
  if (curTick <= loTick) return a0 > 0 ? a0 / (1 / sl - 1 / sh) : 0
  if (curTick >= hiTick) return a1 > 0 ? a1 / (sh - sl) : 0
  const sc = sqrt(curTick)
  const l0 = a0 > 0 ? a0 / (1 / sc - 1 / sh) : Infinity
  const l1 = a1 > 0 ? a1 / (sc - sl) : Infinity
  const L = Math.min(l0, l1)
  return Number.isFinite(L) ? L : 0
}

/**
 * Raw liquidity that deploys the GIVEN holdings' total value at
 * [loTick, hiTick] under the current price — the value-preserving target
 * liquidity for range edits. Holding liquidity constant across a range
 * change does NOT hold value constant: value per unit liquidity,
 * f(lo, hi, cur) = 2√P − P/√hi − √lo, depends on the range (an imbalanced
 * position with one edge near the current price is most sensitive — real
 * AQUA/sUSD shifts leaked 8–54% of position value to the wallet at fixed L).
 * Amounts are linear in raw liquidity, so L* = V(withdrawn) / V(per unit L).
 * Value is measured in human token1 units at the pool's tick price.
 * @param {{amount0: number, amount1: number}} withdrawn - human-unit holdings
 *   (live withdrawal estimate, incl. accrued fees)
 * @param {number} loTick
 * @param {number} hiTick
 * @param {number} curTick - current pool tick (required: sizing without it
 *   silently misprices the position)
 * @param {number} [decimals0=7]
 * @param {number} [decimals1=7]
 * @returns {number} raw u128 liquidity (float — floor at the BigInt boundary)
 * @throws on missing holdings, invalid tick/range, or non-positive value
 */
export function valueMatchedLiquidity(withdrawn, loTick, hiTick, curTick, decimals0 = 7, decimals1 = 7) {
  if (!withdrawn || !Number.isFinite(withdrawn.amount0) || !Number.isFinite(withdrawn.amount1)) {
    throw new Error(`valueMatchedLiquidity: withdrawn holdings required, got ${JSON.stringify(withdrawn)}`)
  }
  if (curTick == null || !Number.isFinite(curTick)) {
    throw new Error(`valueMatchedLiquidity: invalid current tick ${String(curTick)}`)
  }
  const lo = Math.min(loTick, hiTick)
  const hi = Math.max(loTick, hiTick)
  if (!Number.isFinite(lo) || !Number.isFinite(hi) || !(lo < hi)) {
    throw new Error(`valueMatchedLiquidity: invalid range [${String(loTick)}, ${String(hiTick)}]`)
  }
  const price = tickToPriceScaled(curTick, decimals0, decimals1)
  const value = withdrawn.amount0 * price + withdrawn.amount1
  if (!(value > 0)) {
    throw new Error(`valueMatchedLiquidity: non-positive holdings value ${value}`)
  }
  const unit = positionTokenAmounts(1, lo, hi, curTick, decimals0, decimals1)
  const perUnit = unit.amount0 * price + unit.amount1
  if (!(perUnit > 0)) {
    throw new Error(`valueMatchedLiquidity: non-positive value per unit liquidity ${perUnit} over [${lo}, ${hi}] at tick ${curTick}`)
  }
  return value / perUnit
}

/**
 * Stable identity for a concentrated-liquidity position: its tick range.
 * Positions are stored in a refreshable, re-orderable array, so operations
 * must key off the range (not the array index) to avoid acting on a different
 * position after a refresh.
 * @param {{tick_lower: number, tick_upper: number}} pos
 * @returns {string}
 */
export function positionKey(pos) {
  return `${pos?.tick_lower}:${pos?.tick_upper}`
}

/**
 * Value-based asset ratio of a position's two sides at the current price,
 * normalized so the smaller side is 1 — e.g. '1:10' means token0's side is
 * worth 1 unit while token1's side is worth 10 (in token1 terms).
 * price is token1 per token0 (tickToPrice of the current tick).
 * One-sided positions render '1:0' / '0:1'.
 * @param {number|string} amount0 - human units of token0
 * @param {number|string} amount1 - human units of token1
 * @param {number} price - token1 per token0
 * @returns {string|null} 'x:y' ratio, or null when the price is unknown
 */
export function formatAssetRatio(amount0, amount1, price) {
  const p = Number(price)
  if (!isFinite(p) || p <= 0) return null
  const v0 = Math.max(0, Number(amount0) || 0) * p
  const v1 = Math.max(0, Number(amount1) || 0)
  if (v0 <= 0 && v1 <= 0) return '0:0'
  if (v0 <= 0) return '0:1'
  if (v1 <= 0) return '1:0'
  const side = (x) => {
    // up to 3 significant digits, no trailing zeros
    const s = x >= 100 ? Math.round(x).toString() : Number(x.toPrecision(3)).toString()
    return s
  }
  return v0 >= v1 ? `${side(v0 / v1)}:1` : `1:${side(v1 / v0)}`
}

/**
 * Format a single price value.
 * @param {number} p
 * @returns {string}
 */
export function formatPriceValue(p) {
  if (!p || !isFinite(p)) return 'N/A'
  if (p >= 1) return formatNum(p, 4)
  if (p >= 0.0001) return formatNum(p, 7).replace(/(\.\d*?)0+$/, '$1').replace(/\.$/, '')
  return p.toExponential(2)
}

/**
 * Format a price as "X.XXXX token1/token0".
 * @param {number} price
 * @param {string} token0Code
 * @param {string} token1Code
 * @returns {string}
 */
export function formatPrice(price, token0Code, token1Code) {
  if (!price || !isFinite(price)) return 'N/A'
  return `${formatPriceValue(price)} ${token1Code}/${token0Code}`
}

/**
 * Format a price range from tick bounds.
 * @param {number} lowerTick
 * @param {number} upperTick
 * @param {string} token0Code
 * @param {string} token1Code
 * @param {number} [decimals0=7]
 * @param {number} [decimals1=7]
 * @returns {string}
 */
export function formatPriceRange(lowerTick, upperTick, token0Code, token1Code, decimals0 = 7, decimals1 = 7) {
  const lower = tickToPriceScaled(lowerTick, decimals0, decimals1)
  const upper = tickToPriceScaled(upperTick, decimals0, decimals1)
  return `${formatPriceValue(lower)} – ${formatPriceValue(upper)} ${token1Code}/${token0Code}`
}

/**
 * Display code for a token string: 'native' -> 'XLM', 'CODE:ISSUER' -> 'CODE',
 * anything else (bare code, contract id) passthrough.
 * @param {string} t
 * @returns {string}
 */
export function tokenCode(t) {
  if (t === 'native') return 'XLM'
  return (t || '').split(':')[0]
}

/**
 * Format a token string array to display codes.
 * @param {string[]} tokens
 * @returns {string}
 */
export function formatTokenStr(tokens) {
  if (!tokens || !Array.isArray(tokens)) return 'N/A'
  return tokens.map(tokenCode).join(' / ')
}

/**
 * Per-token decimals + tick/price converters for a pool. tokens_decimals is
 * resolved once by fetchPool; absent entries default to classic 7.
 * @returns {{decimals0: number, decimals1: number, decimals: number[],
 *   tokenDecimals: function(number): number, t2p: function, p2t: function,
 *   priceScale: number}}
 */
export function poolDecimals(pool) {
  const d = pool?.tokens_decimals || []
  const decimals0 = d[0] ?? 7
  const decimals1 = d[1] ?? 7
  return {
    decimals0,
    decimals1,
    decimals: [decimals0, decimals1],
    tokenDecimals: (idx) => (idx === 0 ? decimals0 : decimals1),
    t2p: (tick) => tickToPriceScaled(tick, decimals0, decimals1),
    p2t: (price) => priceToTickScaled(price, decimals0, decimals1),
    priceScale: Math.pow(10, decimals0 - decimals1),
  }
}

// Tick spacing is fixed per pool fee tier (Uniswap-v3 style) — verified
// against all Aquarius concentrated pools: 0.10% → 20, 0.30% → 60, 1.00% → 200.
// The pools-list API never returns tick_spacing (only the detail endpoint
// does), so fee is the only spacing hint a list row carries.
const FEE_TO_TICK_SPACING = { 0.0001: 1, 0.0005: 10, 0.001: 20, 0.003: 60, 0.01: 200 }

/** Tick spacing derivable from a pool fee-tier value (e.g. '0.003' → 60). */
export function tickSpacingFromFee(fee) {
  return FEE_TO_TICK_SPACING[parseFloat(fee)] ?? null
}

/**
 * The pool's concentrated tick spacing: its own tick_spacing field when
 * present (pool detail rows), else the fee-tier derivation — the same
 * fallback PoolDetail applies.
 */
export function poolTickSpacing(pool) {
  return pool?.tick_spacing ?? tickSpacingFromFee(pool?.fee)
}

/**
 * Concentrated range width in integer tick-spacing intervals. Position
 * bounds snap to the spacing so the count is integral; rounding guards a
 * straggler bound produced mid-drag.
 */
export function rangeSpacingIntervals(loTick, hiTick, tickSpacing = 1) {
  const s = Math.max(1, tickSpacing || 1)
  return Math.round(Math.abs(hiTick - loTick) / s)
}

/**
 * Concentrated range width as a percent — (price_hi / price_lo − 1) · 100.
 * Prices scale by the same 10^(d0−d1) factor, so raw ticks suffice. Returns
 * the number BEFORE the % sign (formatPctPrecise input); Infinity for a
 * near-full range — render '∞'.
 */
export function rangeWidthPct(loTick, hiTick) {
  return (tickToPrice(Math.abs(hiTick - loTick)) - 1) * 100
}
