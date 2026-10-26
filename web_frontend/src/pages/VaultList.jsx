import { useEffect, useState } from 'react'
import { Link } from 'react-router-dom'
import { VAULTS, expertContractUrl } from '../config'
import { getVaultAssets, getVaultTotalSupply, getStrategyPosition, getPoolSlot0 } from '../services/contracts'
import CopyButton from '../components/CopyButton'
import LoadingGears from '../components/LoadingGears'

const fmt = (n, dec = 7) => {
  if (n === null || n === undefined) return '—'
  const v = Number(n) / 10 ** dec
  return v >= 1000 ? v.toLocaleString(undefined, { maximumFractionDigits: 2 }) : v.toLocaleString(undefined, { maximumFractionDigits: 4 })
}

function VaultRow({ v }) {
  const [data, setData] = useState({})
  useEffect(() => {
    let dead = false
    Promise.allSettled([
      getVaultAssets(v.address),
      getVaultTotalSupply(v.address),
      getStrategyPosition(v.strategy),
      getPoolSlot0(),
    ]).then(([assets, supply, pos, slot0]) => {
      if (dead) return
      const position = pos.status === 'fulfilled' ? pos.value : null
      const cur = slot0.status === 'fulfilled' ? Number(slot0.value?.tick ?? slot0.value?.current_tick) : null
      const tvl = assets.status === 'fulfilled'
        ? Number(assets.value?.[0]?.total_amount ?? assets.value?.[0]?.idle_amount ?? 0) : null
      const inRange = position && cur !== null
        ? cur >= Number(position.tick_lower ?? position.tickLower) && cur <= Number(position.tick_upper ?? position.tickUpper)
        : null
      setData({
        tvl,
        supply: supply.status === 'fulfilled' ? supply.value : null,
        position, cur, inRange,
      })
    })
    return () => { dead = true }
  }, [v.address, v.strategy])

  return (
    <tr>
      <td>
        <div style={{ fontWeight: 'bold' }}>{v.name}</div>
        <div style={{ fontSize: 12, color: '#9977aa' }}>{v.pair}</div>
      </td>
      <td>
        <span className="badge-concentrated">{v.template}</span>{' '}
        <span className="badge-aquarius">{v.venue}</span>
      </td>
      <td>{fmt(data.tvl)} USDC</td>
      <td>{fmt(data.supply)}</td>
      <td>
        {data.inRange === null ? <span style={{ color: '#9977aa' }}>—</span>
          : <span className={data.inRange ? 'in-range' : 'out-of-range'}>{data.inRange ? 'IN RANGE' : 'OUT OF RANGE'}</span>}
      </td>
      <td style={{ fontSize: 12, color: '#9977aa' }}>
        {data.position ? `[${Number(data.position.tick_lower ?? data.position.tickLower)} … ${Number(data.position.tick_upper ?? data.position.tickUpper)}]` : '—'}
      </td>
      <td><Link to={`/vault/${v.address}`}><button className="btn-aqua" style={{ padding: '4px 12px', fontSize: 13 }}>deposit</button></Link></td>
    </tr>
  )
}

export default function VaultList() {
  const [loading, setLoading] = useState(true)
  useEffect(() => { const t = setTimeout(() => setLoading(false), 400); return () => clearTimeout(t) }, [])

  return (
    <div>
      <div style={{ marginBottom: 18 }}>
        <h2 className="text-glow" style={{ marginBottom: 4 }}>Concentrated-liquidity vaults</h2>
        <div style={{ color: '#cc99cc', fontSize: 14 }}>
          Deposit one asset — the vault holds the LP position, keeps it in range, and compounds AQUA + swap fees back in.
        </div>
      </div>

      <div className="synt_tech_card" style={{ borderColor: '#8a6d00', marginBottom: 18, fontSize: 13, color: '#ddbb55' }}>
        ⚠ Testnet smoke deployment — Aquarius pool, Reflector oracle and reward fixtures are mocks.
        No real mainnet assets or production Aquarius positions yet.
      </div>

      {loading ? <LoadingGears /> : (
        <table className="aqua-table" style={{ width: '100%' }}>
          <thead>
            <tr>
              <th>Vault</th><th>Strategy / Venue</th><th>Managed</th><th>Shares</th>
              <th>Status</th><th>Ticks</th><th></th>
            </tr>
          </thead>
          <tbody>
            {VAULTS.map((v) => <VaultRow key={v.address} v={v} />)}
          </tbody>
        </table>
      )}
    </div>
  )
}
