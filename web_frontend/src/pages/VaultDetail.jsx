import { useEffect, useState, useCallback } from 'react'
import { useParams, Link } from 'react-router-dom'
import { VAULTS, expertContractUrl, KNOWN_POOL_POSITIONS } from '../config'
import * as C from '../services/contracts'
import { getTokenBalance } from '../services/contracts'
import { useStore } from '../store'
import CopyButton from '../components/CopyButton'
import LoadingGears from '../components/LoadingGears'
import LiquidityDistribution from '../components/LiquidityDistribution'
import { tickToPrice, formatPriceValue } from '../utils/format'

const DEC = 7
const fmt = (n, d = DEC, max = 4) =>
  n == null ? '—' : (Number(n) / 10 ** d).toLocaleString(undefined, { maximumFractionDigits: max })

function Stat({ label, value, hint }) {
  return (
    <div className="synt_tech_card" style={{ padding: 12 }}>
      <div style={{ fontSize: 12, color: '#9977aa' }}>{label}</div>
      <div style={{ fontSize: 18 }}>{value}</div>
      {hint && <div style={{ fontSize: 11, color: '#775588', marginTop: 2 }}>{hint}</div>}
    </div>
  )
}

export default function VaultDetail() {
  const { address } = useParams()
  const v = VAULTS.find((x) => x.address === address) || VAULTS[0]
  const user = useStore((s) => s.address)
  const setTx = useStore((s) => s.setTx)
  const toast = useStore((s) => s.toast)

  const [d, setD] = useState({})
  const [busy, setBusy] = useState(false)
  const [depAmt, setDepAmt] = useState('')
  const [depToken, setDepToken] = useState('USDC')
  const [wdShares, setWdShares] = useState('')
  const [xlmPct, setXlmPct] = useState(0) // % of withdrawn USDC to swap to XLM
  const [tab, setTab] = useState('deposit')

  const load = useCallback(async () => {
    const [assets, supply, managed, pos, slot0, policy, lastH, myShares, myUsdc, myXlm, poolTok, tickSp, actLiq, pendingRew] = await Promise.allSettled([
      C.getVaultAssets(v.address),
      C.getVaultTotalSupply(v.address),
      C.getTotalManagedFunds(v.address),
      C.getStrategyPosition(v.strategy),
      C.getPoolSlot0(),
      C.getStrategyPolicy(v.strategy),
      C.getLastHarvest(v.strategy),
      user ? C.getVaultShareBalance(user, v.address) : Promise.resolve(0n),
      user ? C.getTokenBalance(C.USDC_SAC, user) : Promise.resolve(0n),
      user ? C.getTokenBalance(C.NATIVE_XLM_SAC, user) : Promise.resolve(0n),
      C.getPoolTokens(),
      C.getTickSpacing(),
      C.getActiveLiquidity(),
      C.getPoolUserReward(v.strategy),
    ])
    const val = (r) => (r.status === 'fulfilled' ? r.value : null)
    const position = val(pos)
    const slot = val(slot0)
    const cur = slot ? Number(slot.tick ?? slot.current_tick) : null
    const toks = val(poolTok) ?? []
    const tokIdx = {
      usdc: toks.findIndex((t) => t === C.USDC_SAC),
      xlm: toks.findIndex((t) => t === C.NATIVE_XLM_SAC),
    }
    let posDetail = null
    if (position) {
      const tl = Number(position.tick_lower ?? position.tickLower)
      const tu = Number(position.tick_upper ?? position.tickUpper)
      const [pp, fees] = await Promise.all([
        C.getPoolPosition(v.strategy, tl, tu).catch(() => null),
        C.getPositionFees(v.strategy, tl, tu).catch(() => null),
      ])
      posDetail = { pp, fees }
    }
    setD({
      assets: val(assets), supply: val(supply), managed: val(managed),
      position, posDetail, cur, slot, policy: val(policy), lastH: val(lastH),
      myShares: val(myShares), myUsdc: val(myUsdc), myXlm: val(myXlm),
      tickSpacing: val(tickSp), activeLiq: val(actLiq), pendingReward: val(pendingRew),
      tokIdx: tokIdx.usdc >= 0 && tokIdx.xlm >= 0 ? tokIdx : null,
    })
  }, [v.address, v.strategy, user])

  useEffect(() => { load().catch((e) => toast({ type: 'error', text: `load failed: ${e.message}` })) }, [load])

  const doTx = async (title, fn) => {
    setBusy(true)
    setTx({ title, status: 'simulating' })
    try {
      const { hash } = await fn({ onStatus: (s) => setTx({ title, status: s }) })
      setTx({ title, status: 'success', hash })
      toast({ type: 'success', text: `${title} confirmed`, link: expertContractUrl('') + `/../../tx/${hash}` })
      load()
    } catch (e) {
      setTx({ title, status: 'error', error: e.message || String(e) })
    } finally {
      setBusy(false)
    }
  }

  const onDeposit = async () => {
    const amt = BigInt(Math.round(parseFloat(depAmt || '0') * 10 ** DEC))
    if (amt <= 0n) return toast({ type: 'error', text: `enter a ${depToken} amount` })
    if (depToken === 'USDC') {
      doTx(`deposit ${depAmt} USDC`, (o) => C.vaultDeposit({ from: user, amountUsdc: amt, minUsdc: 0n, onStatus: o?.onStatus }))
      return
    }
    // zap: swap depToken → USDC in the same tx, deposit the guaranteed out_min
    try {
      const est = await C.estimatePoolSwap(d.tokIdx[depToken.toLowerCase()], d.tokIdx.usdc, amt)
      const outMin = (BigInt(est ?? amt) * 99n) / 100n
      doTx(`deposit ${depAmt} ${depToken} → USDC`, (o) =>
        C.depositZap({ from: user, token: depToken, amount: amt, outMin, tokIdx: d.tokIdx, onStatus: o?.onStatus }))
    } catch (e) {
      toast({ type: 'error', text: `swap quote failed: ${e.message}` })
    }
  }
  const onWithdraw = async () => {
    const sh = BigInt(Math.round(parseFloat(wdShares || '0') * 10 ** DEC))
    if (sh <= 0n) return toast({ type: 'error', text: 'enter share amount' })
    const label = xlmPct > 0 ? `withdraw ${wdShares} shares → ${100 - xlmPct}% USDC + ${xlmPct}% XLM` : `withdraw ${wdShares} shares`
    doTx(label, (o) => C.withdrawSplit({ from: user, shares: sh, minUsdc: 0n, swapPct: xlmPct, tokIdx: d.tokIdx ?? {}, onStatus: o?.onStatus }))
  }

  const tvl = d.managed?.[0]?.total_amount ?? d.assets?.[0]?.total_amount
  const tl = d.position ? Number(d.position.tick_lower ?? d.position.tickLower) : null
  const tu = d.position ? Number(d.position.tick_upper ?? d.position.tickUpper) : null
  const inRange = tl !== null && d.cur !== null && d.cur !== undefined ? d.cur >= tl && d.cur <= tu : null

  // Pool-liquidity histogram: Uniswap-V3 liquidity_net per tick, built from
  // the known on-chain positions + the strategy's live position.
  const stratLiq = Number(d.posDetail?.pp?.[0] ?? d.position?.liquidity ?? 0)
  const tickMap = {}
  const allPos = tl !== null && stratLiq > 0
    ? [...KNOWN_POOL_POSITIONS, { tick_lower: tl, tick_upper: tu, liquidity: stratLiq }]
    : KNOWN_POOL_POSITIONS
  allPos.forEach((p) => {
    tickMap[p.tick_lower] = (tickMap[p.tick_lower] ?? 0) + Number(p.liquidity)
    tickMap[p.tick_upper] = (tickMap[p.tick_upper] ?? 0) - Number(p.liquidity)
  })

  // The mock pool stores deposit amounts statically; a real CL position's
  // token mix drifts with price (all token0 below the band, all token1
  // above it). Derive the implied split for display: V3 amounts per unit
  // liquidity at the live spot, rescaled so implied spot value ties to the
  // contract's stored deposits.
  const impliedHoldings = (() => {
    const pp = d.posDetail?.pp
    if (tl === null || tu === null || d.cur == null || !pp) return null
    const a0s = Number(pp[1])
    const a1s = Number(pp[2])
    const sqrt96 = Number(d.slot?.sqrt_price_x96 ?? 0)
    if (!(sqrt96 > 0)) return { xlm: a0s, usdc: a1s }
    const sP = sqrt96 / (2 ** 96)
    const sa = sP / Math.pow(1.0001, (d.cur - tl) / 2)
    const sb = sP * Math.pow(1.0001, (tu - d.cur) / 2)
    const P = sP * sP
    const [u0, u1] = sP <= sa ? [1 / sa - 1 / sb, 0]
      : sP >= sb ? [0, sb - sa]
      : [1 / sP - 1 / sb, sP - sa]
    const impliedVal = u0 * P + u1
    const k = impliedVal > 0 ? (a0s * P + a1s) / impliedVal : 0
    return { xlm: u0 * k, usdc: u1 * k }
  })()

  return (
    <div>
      <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'baseline', flexWrap: 'wrap', gap: 8 }}>
        <div>
          <h2 className="text-glow" style={{ marginBottom: 4 }}>{v.name}</h2>
          <div style={{ color: '#cc99cc', fontSize: 14 }}>
            {v.pair} · <span className="badge-concentrated">{v.template}</span> <span className="badge-aquarius">{v.venue}</span>
            {inRange !== null && <> <span className={inRange ? 'in-range' : 'out-of-range'}>{inRange ? 'IN RANGE' : 'OUT OF RANGE'}</span></>}
          </div>
        </div>
        <div style={{ fontSize: 12, color: '#9977aa', display: 'flex', gap: 10, alignItems: 'center' }}>
          <a href={expertContractUrl(v.address)} target="_blank" rel="noreferrer" style={{ color: '#a020f0' }}>{v.address.slice(0, 10)}…</a>
          <CopyButton text={v.address} />
        </div>
      </div>

      <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fit, minmax(150px, 1fr))', gap: 10, margin: '16px 0' }}>
        <Stat label="Total managed" value={`${fmt(tvl)} USDC`} />
        <Stat label="Share supply" value={fmt(d.supply)} hint="dfToken = vault itself" />
        <Stat label="Pool tick" value={d.cur ?? '—'} hint={d.cur != null ? `price ${formatPriceValue(tickToPrice(d.cur))} USDC/XLM` : undefined} />
        <Stat label="Position band" value={tl !== null ? `[${tl} … ${tu}]` : '—'} hint={tl !== null ? `${formatPriceValue(tickToPrice(tl))} – ${formatPriceValue(tickToPrice(tu))}` : 'no open position'} />
        <Stat label="Active liquidity" value={d.activeLiq != null ? fmt(d.activeLiq, 7, 2) : '—'} hint={d.tickSpacing != null ? `tick spacing ${d.tickSpacing}` : undefined} />
        <Stat label="Pending AQUA" value={d.pendingReward != null ? fmt(d.pendingReward, 7, 4) : '—'} hint="auto-compounds on harvest" />
        <Stat label="Your shares" value={fmt(d.myShares)} hint={user ? `~${fmt(d.myShares)} USDC` : 'connect wallet'} />
      </div>

      <LiquidityDistribution
        title="POSITION vs PRICE — the band the keeper keeps in range"
        currentTick={d.cur ?? 0}
        activeLiquidity={Number(d.activeLiq ?? 0)}
        tickMap={tickMap}
        positions={tl !== null ? [{
          tick_lower: tl, tick_upper: tu,
          liquidity: stratLiq || 1,
        }] : []}
        mode="pool"
        showModeToggle
        highlightIdx={0}
        token0Code="XLM"
        token1Code="USDC"
        tickSpacing={Number(d.tickSpacing ?? 1) || 1}
        height={190}
        initialWindowPct={0.4}
      />

      {tl !== null && d.posDetail?.pp && (
        <div className="synt_tech_card" style={{ margin: '10px 0', padding: '10px 14px', fontSize: 13, display: 'flex', gap: 24, flexWrap: 'wrap' }}>
          <span style={{ color: '#9977aa' }}>POSITION HOLDINGS</span>
          <span>XLM: <b>{fmt(impliedHoldings?.xlm ?? d.posDetail.pp[1])}</b></span>
          <span>USDC: <b>{fmt(impliedHoldings?.usdc ?? d.posDetail.pp[2])}</b></span>
          {d.posDetail.fees && <span>uncollected fees: <b>{fmt(d.posDetail.fees[0])} XLM</b> + <b>{fmt(d.posDetail.fees[1])} USDC</b></span>}
          <span style={{ color: '#775588' }}>liquidity {fmt(d.posDetail.pp[0])}</span>
        </div>
      )}

      <div style={{ display: 'grid', gridTemplateColumns: 'minmax(300px, 380px) 1fr', gap: 14, alignItems: 'start' }}>
        <div className="synt_tech_card">
          <div style={{ display: 'flex', gap: 8, marginBottom: 14 }}>
            {['deposit', 'withdraw'].map((t) => (
              <button key={t} className={tab === t ? 'btn-aqua' : 'btn-aqua'} onClick={() => setTab(t)}
                style={{ opacity: tab === t ? 1 : 0.45, flex: 1 }}>{t}</button>
            ))}
          </div>
          {tab === 'deposit' ? (
            <>
              <div style={{ display: 'flex', gap: 6, marginBottom: 8 }}>
                {['USDC', 'XLM'].map((t) => (
                  <button key={t} className="btn-aqua" onClick={() => setDepToken(t)}
                    style={{ flex: 1, padding: '4px 0', fontSize: 13, opacity: depToken === t ? 1 : 0.4 }}>{t}</button>
                ))}
              </div>
              <div style={{ fontSize: 13, color: '#9977aa', marginBottom: 6 }}>
                {depToken} balance: {user ? fmt(depToken === 'USDC' ? d.myUsdc : d.myXlm) : '—'}
              </div>
              <input className="synt-input" type="number" min="0" placeholder="0.0" value={depAmt}
                onChange={(e) => setDepAmt(e.target.value)} style={{ width: '100%', marginBottom: 10 }} />
              <button className="btn-aqua" style={{ width: '100%' }} disabled={!user || busy || (depToken !== 'USDC' && !d.tokIdx)} onClick={onDeposit}>
                {user ? (depToken === 'USDC' ? 'deposit USDC' : `swap ${depToken} → USDC + deposit`) : 'connect wallet to deposit'}
              </button>
              <div style={{ fontSize: 12, color: '#775588', marginTop: 8 }}>
                {depToken === 'USDC'
                  ? 'you receive vault shares — value accrues as the position earns fees + compounded AQUA'
                  : `one tx: pool swap ${depToken}→USDC, then deposit — you receive the same USDC-denominated shares`}
              </div>
            </>
          ) : (
            <>
              <div style={{ fontSize: 13, color: '#9977aa', marginBottom: 6 }}>
                your shares: {user ? fmt(d.myShares) : '—'}
              </div>
              <input className="synt-input" type="number" min="0" placeholder="0.0" value={wdShares}
                onChange={(e) => setWdShares(e.target.value)} style={{ width: '100%', marginBottom: 6 }} />
              <button className="btn-aqua" style={{ fontSize: 12, padding: '2px 8px', marginBottom: 10 }}
                onClick={() => d.myShares && setWdShares((Number(d.myShares) / 10 ** DEC).toString())}>max</button>
              <div style={{ fontSize: 13, color: '#9977aa', marginBottom: 4 }}>
                receive as: <span style={{ color: '#C1A100' }}>{100 - xlmPct}% USDC</span> + <span style={{ color: '#a020f0' }}>{xlmPct}% XLM</span>
              </div>
              <input type="range" min="0" max="100" step="5" value={xlmPct} disabled={!d.tokIdx}
                onChange={(e) => setXlmPct(Number(e.target.value))} style={{ width: '100%', marginBottom: 10 }} />
              <button className="btn-aqua" style={{ width: '100%' }} disabled={!user || busy} onClick={onWithdraw}>
                {user ? 'withdraw' : 'connect wallet to withdraw'}
              </button>
              <div style={{ fontSize: 12, color: '#775588', marginTop: 8 }}>
                one tx: shares burn → USDC back{xlmPct > 0 ? ', then the XLM share swaps in the same tx' : ' to your wallet'}
              </div>
            </>
          )}
        </div>

        <div className="synt_tech_card">
          <div style={{ fontSize: 12, color: '#9977aa', marginBottom: 8 }}>HOW THIS VAULT EARNS</div>
          <ul style={{ margin: 0, paddingLeft: 18, fontSize: 14, lineHeight: 1.7 }}>
            <li>Your USDC sits inside a concentrated-liquidity position on Aquarius.</li>
            <li>A keeper submits rebalance plans; on-chain policy validates every plan (slippage, tick bounds, loss cap, cooldown).</li>
            <li>Swap fees + AQUA rewards are claimed and <b>compounded back into the position</b> — no separate claim needed; value shows up in your share price.</li>
            <li>15% performance fee on harvested income, split platform / template author / vault operator.</li>
          </ul>
          <div style={{ fontSize: 12, color: '#9977aa', marginTop: 14, marginBottom: 6 }}>STRATEGY</div>
          <div style={{ fontSize: 13, display: 'flex', gap: 8, alignItems: 'center' }}>
            <span>cl_aquarius</span>
            <a href={expertContractUrl(v.strategy)} target="_blank" rel="noreferrer" style={{ color: '#a020f0' }}>{v.strategy.slice(0, 10)}…</a>
            <CopyButton text={v.strategy} />
          </div>
          <div style={{ fontSize: 12, color: '#9977aa', marginTop: 8, marginBottom: 4 }}>POOL (XLM/USDC · Aquarius)</div>
          <div style={{ fontSize: 13, display: 'flex', gap: 8, alignItems: 'center' }}>
            <a href={expertContractUrl(C.POOL)} target="_blank" rel="noreferrer" style={{ color: '#a020f0' }}>{C.POOL.slice(0, 10)}…</a>
            <CopyButton text={C.POOL} />
            {d.cur != null && <span style={{ color: '#775588' }}>tick {d.cur} · {formatPriceValue(tickToPrice(d.cur))} USDC/XLM</span>}
          </div>
          {d.policy && (
            <div style={{ fontSize: 12, color: '#775588', marginTop: 8 }}>
              policy: max slip {Number(d.policy.max_slippage_bps)} bps · loss cap {Number(d.policy.max_loss_bps_per_window)} bps/{Number(d.policy.loss_window_secs)}s · cooldown {Number(d.policy.cooldown_secs)}s · perf fee {Number(d.policy.perf_fee_bps) / 100}%
            </div>
          )}
          {d.lastH != null && <div style={{ fontSize: 12, color: '#775588', marginTop: 4 }}>last harvest: {new Date(Number(d.lastH) * 1000).toLocaleString()}</div>}
        </div>
      </div>
    </div>
  )
}
