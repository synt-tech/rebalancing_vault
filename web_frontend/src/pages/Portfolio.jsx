import { useEffect, useState } from 'react'
import { Link } from 'react-router-dom'
import { VAULTS } from '../config'
import * as C from '../services/contracts'
import { useStore } from '../store'
import LoadingGears from '../components/LoadingGears'

const fmt = (n, d = 7) => (n == null ? '—' : (Number(n) / 10 ** d).toLocaleString(undefined, { maximumFractionDigits: 4 }))

export default function Portfolio() {
  const user = useStore((s) => s.address)
  const connect = useStore((s) => s.connect)
  const [rows, setRows] = useState(null)

  useEffect(() => {
    if (!user) { setRows(null); return }
    Promise.all(VAULTS.map(async (v) => {
      try {
        const shares = await C.getVaultShareBalance(user, v.address)
        let usdc = null
        if (shares > 0n) {
          const amounts = await C.getAssetAmountsPerShares(shares, v.address)
          usdc = Array.isArray(amounts) ? amounts[0] : amounts
        }
        return { v, shares, usdc }
      } catch { return { v, shares: 0n, usdc: null } }
    })).then(setRows)
  }, [user])

  if (!user) {
    return (
      <div className="synt_tech_card" style={{ textAlign: 'center', padding: 40 }}>
        <div style={{ marginBottom: 14, color: '#cc99cc' }}>connect a wallet to see your vault positions</div>
        <button className="btn-aqua" onClick={connect}>connect wallet</button>
      </div>
    )
  }
  if (!rows) return <LoadingGears />

  return (
    <div>
      <h2 className="text-glow">My positions</h2>
      <table className="aqua-table" style={{ width: '100%', marginTop: 12 }}>
        <thead><tr><th>Vault</th><th>Shares</th><th>Value (USDC)</th><th></th></tr></thead>
        <tbody>
          {rows.map(({ v, shares, usdc }) => (
            <tr key={v.address}>
              <td>{v.name} <span style={{ fontSize: 12, color: '#9977aa' }}>{v.pair}</span></td>
              <td>{fmt(shares)}</td>
              <td>{usdc == null ? '—' : fmt(usdc)}</td>
              <td><Link to={`/vault/${v.address}`}><button className="btn-aqua" style={{ padding: '4px 12px', fontSize: 13 }}>manage</button></Link></td>
            </tr>
          ))}
        </tbody>
      </table>
      <div style={{ fontSize: 12, color: '#775588', marginTop: 10 }}>
        AQUA rewards compound into share value on each harvest — there is no separate per-user reward claim in v1.
      </div>
    </div>
  )
}
