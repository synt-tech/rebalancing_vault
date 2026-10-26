/**
 * Shared token-amount input handling — THE parsing/formatting helpers every
 * amount field goes through (see components/AmountInput.jsx).
 *
 * The app's display convention (utils/format.js `formatNum`) groups thousands
 * with an apostrophe: 26195.8401005 renders as "26'195.8401005". Users paste
 * and copy those values back into inputs, so every amount field must accept
 * the separator — these helpers canonicalize free-form input text into the
 * plain numeric strings transaction builders expect.
 *
 * Canonical form: ASCII digits with at most one '.', optional scientific
 * exponent ("1e-5" — formatPriceValue emits exponential notation for tiny
 * prices, so pasting it back must round-trip). No separators, no spaces.
 * `sanitizeAmountText` preserves mid-typing states ("", "12.", "0.000");
 * `normalizeAmountText` additionally trims for commit (blur/submit).
 */

import { groupThousands } from './format'

/**
 * Strip formatting characters (' separators, whitespace, commas) and invalid
 * input from a user-typed amount, keeping a canonical numeric string.
 * Preserves intermediate editing states: empty, trailing dot, leading zeros,
 * in-progress exponent.
 *
 * @param {string|number|null} text   raw input text
 * @param {number|null} maxDecimals   cap on fraction digits (token decimals)
 * @returns {string} canonical numeric string (possibly '' / '12.' / '.5')
 */
export function sanitizeAmountText(text, maxDecimals = null) {
  if (text == null) return ''
  let s = String(text).replace(/['\s,_]/g, '')

  // Optional exponent tail: e/E, optional sign, digits — kept verbatim so
  // typing "1e" or "1e-" doesn't mangle the mantissa mid-keystroke.
  let exp = ''
  const ei = s.search(/[eE]/)
  if (ei !== -1) {
    const tail = s.slice(ei + 1)
    const sign = tail.trimStart().startsWith('-') ? '-' : ''
    exp = 'e' + sign + tail.replace(/[^0-9]/g, '')
    s = s.slice(0, ei)
  }

  s = s.replace(/[^0-9.]/g, '')
  const dot = s.indexOf('.')
  if (dot !== -1) {
    let frac = s.slice(dot + 1).replace(/\./g, '')
    if (maxDecimals != null) frac = frac.slice(0, maxDecimals)
    s = s.slice(0, dot) + '.' + frac
  }
  return s + exp
}

/**
 * Canonicalize for commit (blur / programmatic set): sanitized, then trailing
 * fraction zeros and a dangling '.' removed. Preserves '0'-style mantissas so
 * '' stays '' rather than collapsing to a fake value.
 */
export function normalizeAmountText(text, maxDecimals = null) {
  const s = sanitizeAmountText(text, maxDecimals)
  const ei = s.search(/[eE]/)
  const mant = ei === -1 ? s : s.slice(0, ei)
  const exp = ei === -1 ? '' : s.slice(ei)
  const dot = mant.indexOf('.')
  if (dot === -1) return mant + exp
  const frac = mant.slice(dot + 1).replace(/0+$/, '')
  const intPart = mant.slice(0, dot)
  return (frac === '' ? intPart : `${intPart}.${frac}`) + exp
}

/**
 * Insert ' thousands separators into the integer part of a numeric string.
 * Idempotent — existing separators are stripped first, so it is safe on values
 * already formatted by formatNum/formatPriceValue. Fraction and exponent pass
 * through unchanged; non-numeric garbage sanitizes first.
 */
export function groupAmountText(text) {
  const s = sanitizeAmountText(text)
  if (s === '') return ''
  const ei = s.search(/[eE]/)
  const mant = ei === -1 ? s : s.slice(0, ei)
  const exp = ei === -1 ? '' : s.slice(ei)
  const dot = mant.indexOf('.')
  const intPart = dot === -1 ? mant : mant.slice(0, dot)
  const grouped = groupThousands(intPart)
  return grouped + (dot === -1 ? '' : mant.slice(dot)) + exp
}

/**
 * Caret index into a grouped amount string such that `n` canonical
 * (non-separator) characters precede it. Keeps the caret glued to the digit
 * it followed while ' separators shift around it — the live-regrouping
 * companion to groupAmountText (see components/AmountInput.jsx).
 */
export function caretAfterCanonicalChars(text, n) {
  let i = 0, seen = 0
  while (i < text.length && seen < n) {
    if (text[i] !== "'") seen++
    i++
  }
  return i
}

/**
 * Parse amount input text to a finite number, or NaN. Accepts ' separators,
 * whitespace, and exponential notation.
 */
export function parseAmount(text) {
  const s = sanitizeAmountText(text)
  if (s === '' || s === '.') return NaN
  const n = Number(s)
  return Number.isFinite(n) ? n : NaN
}
