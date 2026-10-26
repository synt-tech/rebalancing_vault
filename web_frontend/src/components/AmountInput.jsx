import { useEffect, useRef, useState } from 'react'
import {
  sanitizeAmountText,
  normalizeAmountText,
  groupAmountText,
  caretAfterCanonicalChars,
} from '../utils/amountInput'
import { useIsomorphicLayoutEffect } from './UiControls'

/**
 * THE shared token-amount input — a drop-in <input> replacement for every
 * field that takes a token amount (swaps, deposits, withdrawals, liquidity,
 * order size, claim thresholds, ...). Do NOT add another <input type="text">
 * or local clamp/normalize helper for amounts — use this.
 *
 * Contract:
 *  - `value` is the canonical numeric string (or number) — digits + '.',
 *    NO ' separators. Whatever your handler stores is parseFloat/BigInt-safe.
 *  - `onChange(canonical, event)` receives the sanitized value on every
 *    keystroke; mid-typing states ('', '12.', '0.000') are preserved.
 *  - The displayed text is grouped LIVE ("1234567" renders "1'234'567" as it
 *    is typed, matching formatNum's convention) — the caret is re-anchored by
 *    counting canonical chars so regrouping never corrupts an edit in
 *    progress. Deleting a ' separator removes the neighbouring digit
 *    (currency-input convention): backspace eats the digit to its left,
 *    forward-delete the digit to its right.
 *  - The user may type or paste ' separators, whitespace, commas, and
 *    exponential notation — all canonicalized away in the emitted value.
 *  - `maxDecimals` caps fraction digits (pass the token's decimals).
 *  - `normalizeOnBlur` (default) commits a tidied value on blur: trailing
 *    zeros and a dangling '.' removed.
 */
export default function AmountInput({
  value,
  onChange,
  maxDecimals = null,
  groupSeparators = true,
  normalizeOnBlur = true,
  ...rest
}) {
  const [focused, setFocused] = useState(false)
  const [raw, setRaw] = useState('')
  // Last canonical value this input emitted — distinguishes "parent echoing
  // our own onChange" from an external programmatic set while focused.
  const lastEmitted = useRef(null)
  const inputRef = useRef(null)
  // Text most recently committed into the DOM — needed to detect separator
  // deletion on the next keystroke (the canonical value doesn't change).
  const lastText = useRef('')
  // Caret position to restore after the regrouped text commits.
  const pendingCaret = useRef(null)

  const displayValue = (v) =>
    groupSeparators ? groupAmountText(v ?? '') : normalizeAmountText(v ?? '')

  const display = focused ? raw : displayValue(value)

  useIsomorphicLayoutEffect(() => {
    lastText.current = display
    if (pendingCaret.current != null && inputRef.current) {
      inputRef.current.setSelectionRange(pendingCaret.current, pendingCaret.current)
      pendingCaret.current = null
    }
  })

  // Programmatic value changes while focused (max buttons, quote fills on
  // the paired field) must replace the text being edited.
  useEffect(() => {
    if (!focused) return
    const canonical = normalizeAmountText(value ?? '', maxDecimals)
    if (canonical === lastEmitted.current) return
    lastEmitted.current = canonical
    setRaw(displayValue(value))
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [value])

  return (
    <input
      {...rest}
      ref={inputRef}
      type="text"
      inputMode={rest.inputMode ?? 'decimal'}
      value={display}
      onFocus={(e) => {
        lastEmitted.current = null
        setRaw(displayValue(value))
        setFocused(true)
        rest.onFocus?.(e)
      }}
      onChange={(e) => {
        const el = e.target
        const prevText = lastText.current
        const caret = el.selectionStart ?? el.value.length
        let canonical = sanitizeAmountText(el.value, maxDecimals)
        // canonical chars before the caret — its anchor in the regrouped text
        let caretCanon = sanitizeAmountText(el.value.slice(0, caret), maxDecimals).length
        if (
          canonical !== '' &&
          canonical === sanitizeAmountText(prevText, maxDecimals) &&
          el.value.length < prevText.length
        ) {
          // A ' separator was deleted without changing the value — remove the
          // neighbouring digit instead so the keystroke stays useful.
          if (e.nativeEvent?.inputType === 'deleteContentForward') {
            canonical = canonical.slice(0, caretCanon) + canonical.slice(caretCanon + 1)
          } else {
            canonical = canonical.slice(0, caretCanon - 1) + canonical.slice(caretCanon)
            caretCanon = Math.max(0, caretCanon - 1)
          }
        }
        const text = groupSeparators ? groupAmountText(canonical) : canonical
        pendingCaret.current =
          text === el.value ? null : caretAfterCanonicalChars(text, caretCanon)
        lastEmitted.current = normalizeAmountText(canonical, maxDecimals)
        setRaw(text)
        onChange?.(canonical, e)
      }}
      onBlur={(e) => {
        setFocused(false)
        if (normalizeOnBlur) {
          const canonical = normalizeAmountText(raw, maxDecimals)
          if (canonical !== normalizeAmountText(value ?? '', maxDecimals)) {
            onChange?.(canonical, e)
          }
        }
        rest.onBlur?.(e)
      }}
    />
  )
}
