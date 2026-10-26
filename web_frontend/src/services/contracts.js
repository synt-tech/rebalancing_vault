// Typed wrappers over the deployed platform contracts (addresses: config.js).
// Token amounts are stroops-style i128 as BigInt; display layer formats.

import { Address, nativeToScVal, xdr, scValToNative, Contract } from '@stellar/stellar-sdk'
import { simulateView, readStorage, symKey, submitContractCall, submitOps } from './soroban'
import { VAULT, POOL, USDC_SAC, XLM_SAC } from '../config'

const addr = (a) => nativeToScVal(a, { type: 'address' })
const i128 = (n) => nativeToScVal(BigInt(n), { type: 'i128' })
const u32 = (n) => nativeToScVal(n, { type: 'u32' })
const vecI128 = (arr) =>
  xdr.ScVal.scvVec(arr.map((n) => nativeToScVal(BigInt(n), { type: 'i128' })))

// ---- vault (DeFindex vault — also IS the share token) ----
export const getVaultAssets = (v = VAULT.address) => simulateView(v, 'get_assets')
export const getVaultTotalSupply = (v = VAULT.address) => simulateView(v, 'total_supply')
export const getVaultShareBalance = (user, v = VAULT.address) =>
  simulateView(v, 'balance', [addr(user)])
export const getVaultSymbol = (v = VAULT.address) => simulateView(v, 'symbol')
export const getVaultDecimals = (v = VAULT.address) => simulateView(v, 'decimals')
// shares -> underlying quote (deposit/withdraw preview + position value)
export const getAssetAmountsPerShares = (dfAmount, v = VAULT.address) =>
  simulateView(v, 'get_asset_amounts_per_shares', [i128(dfAmount)])
export const getTotalManagedFunds = (v = VAULT.address) =>
  simulateView(v, 'fetch_total_managed_funds')

// ---- strategy (cl_aquarius) ----
export const getStrategyBalance = (s = VAULT.strategy, from = VAULT.address) =>
  simulateView(s, 'balance', [addr(from)])
export const getStrategyAsset = (s = VAULT.strategy) => simulateView(s, 'asset')
// Position {tick_lower, tick_upper} lives in instance storage DataKey::Position
export const getStrategyPosition = (s = VAULT.strategy) => readStorage(s, symKey('Position'))
export const getStrategyPolicy = (s = VAULT.strategy) => readStorage(s, symKey('Policy'))
export const getLastHarvest = (s = VAULT.strategy) => readStorage(s, symKey('LastHarvest'))

// ---- pool (mock CL pool — same interface as Aquarius) ----
export const getPoolSlot0 = (p = POOL) => simulateView(p, 'get_slot0')
export const getPoolTokens = (p = POOL) => simulateView(p, 'get_tokens')
export const estimatePoolSwap = (inIdx, outIdx, amount, p = POOL) =>
  simulateView(p, 'estimate_swap', [u32(inIdx), u32(outIdx), nativeToScVal(BigInt(amount), { type: 'u128' })])
export const getPositionFees = (owner, tickLower, tickUpper, p = POOL) =>
  simulateView(p, 'get_position_fees', [addr(owner), nativeToScVal(tickLower, { type: 'i32' }), nativeToScVal(tickUpper, { type: 'i32' })])
export const getActiveLiquidity = (p = POOL) => simulateView(p, 'get_active_liquidity')
export const getTickSpacing = (p = POOL) => simulateView(p, 'get_tick_spacing')
export const getPoolUserReward = (owner, p = POOL) => simulateView(p, 'get_user_reward', [addr(owner)])
// (liq, amount0, amount1) of the position at [tl, tu] owned by `owner`
export const getPoolPosition = (owner, tickLower, tickUpper, p = POOL) =>
  simulateView(p, 'get_position', [addr(owner), nativeToScVal(tickLower, { type: 'i32' }), nativeToScVal(tickUpper, { type: 'i32' })])

// testnet XLM SAC from DEPLOYMENTS.md (Asset.native().contractId() yields mainnet id)
export const NATIVE_XLM_SAC = XLM_SAC
export { USDC_SAC, POOL }

// ---- token SAC ----
export const getTokenBalance = (token, user) => simulateView(token, 'balance', [addr(user)])

// ---- writes ----
const u128 = (n) => nativeToScVal(BigInt(n), { type: 'u128' })
const bool = (b) => nativeToScVal(!!b, { type: 'bool' })

const depositOp = (from, amountUsdc, minUsdc, vault = VAULT.address, invest = true) =>
  new Contract(vault).call('deposit', vecI128([amountUsdc]), vecI128([minUsdc ?? 0]), addr(from), bool(invest))

const withdrawOp = (from, shares, minOut, vault = VAULT.address) =>
  new Contract(vault).call('withdraw', i128(shares), vecI128([minOut ?? 0]), addr(from))

const swapOp = (user, inIdx, outIdx, inAmount, outMin, p = POOL) =>
  new Contract(p).call('swap', addr(user), u32(inIdx), u32(outIdx), u128(inAmount), u128(outMin))

export function vaultDeposit({ from, amountUsdc, minUsdc, onStatus }) {
  return submitOps({ from, ops: [depositOp(from, amountUsdc, minUsdc)], onStatus })
}
export function vaultWithdraw({ from, shares, minOut, onStatus }) {
  return submitOps({ from, ops: [withdrawOp(from, shares, minOut)], onStatus })
}

// Deposit either pool token: XLM (or other side) swaps → USDC, then deposits —
// all in one atomic tx so a failed deposit can't strand the swapped USDC.
// tokIdx: {xlm: pool token index of XLM, usdc: index of USDC}
export async function depositZap({ from, token, amount, outMin, tokIdx, onStatus }) {
  if (token === 'USDC') {
    return submitOps({ from, onStatus, ops: [depositOp(from, amount, outMin)] })
  }
  const swapIn = tokIdx[token.toLowerCase()]
  const ops = [
    swapOp(from, swapIn, tokIdx.usdc, amount, outMin),
    depositOp(from, outMin, 0n),
  ]
  return submitOps({ from, ops, onStatus })
}

// Withdraw shares → USDC lands at `from`; an optional second op swaps a chosen
// fraction of it to XLM — user picks the split, still one atomic tx.
// swapPct = percent (0..100) of the withdrawn USDC to convert to XLM.
// Everything is sized off a withdraw SIMULATION, not the NAV preview:
// get_asset_amounts_per_shares prices shares at oracle NAV, but the real
// unwind eats pool slippage (~5.5x less on the thin mock pool), so a swap leg
// sized on the preview would spend USDC the user never receives — reverts on
// real SACs once balance checks apply. We floor withdraw's min_amounts_out at
// 99.5% of the simulated payout and size the swap under that floor: a
// last-second state change then reverts at the withdraw's own min-out instead
// of over-spending at the swap.
export async function withdrawSplit({ from, shares, minUsdc, swapPct, tokIdx, onStatus, vault = VAULT.address }) {
  const simOut = await simulateView(vault, 'withdraw', [
    i128(shares), vecI128([minUsdc ?? 0]), addr(from),
  ])
  const usdcOut = BigInt(Array.isArray(simOut) ? simOut[0] : simOut)
  const floor = (usdcOut * 995n) / 1000n
  const minOut = BigInt(minUsdc ?? 0n) > floor ? BigInt(minUsdc) : floor

  const ops = [withdrawOp(from, shares, minOut, vault)]
  const pct = Math.round(Number(swapPct) || 0)
  if (pct > 0) {
    if (!(tokIdx?.usdc >= 0 && tokIdx?.xlm >= 0)) throw new Error('pool token indices unavailable')
    const swapUsdc = (floor * BigInt(pct)) / 100n
    if (swapUsdc > 0n) {
      const est = await estimatePoolSwap(tokIdx.usdc, tokIdx.xlm, swapUsdc)
      const xlmMinOut = (BigInt(est ?? swapUsdc) * 99n) / 100n
      ops.push(swapOp(from, tokIdx.usdc, tokIdx.xlm, swapUsdc, xlmMinOut))
    }
  }
  return submitOps({ from, ops, onStatus })
}
