import { useState } from 'react'
import { Link, NavLink } from 'react-router-dom'
import { useStore } from '../store'
import Identicon from './Identicon'
import CopyButton from './CopyButton'

const short = (a) => (a ? `${a.slice(0, 6)}…${a.slice(-4)}` : '')

export default function Layout({ children }) {
  const address = useStore((s) => s.address)
  const connecting = useStore((s) => s.connecting)
  const connect = useStore((s) => s.connect)
  const connectLocal = useStore((s) => s.connectLocal)
  const disconnect = useStore((s) => s.disconnect)
  const toast = useStore((s) => s.toast)

  const [keyOpen, setKeyOpen] = useState(false)
  const [secret, setSecret] = useState('')

  const onImportKey = async () => {
    try {
      await connectLocal(secret)
      setSecret('')
      setKeyOpen(false)
    } catch (e) {
      toast({ type: 'error', text: `invalid secret key: ${e.message}` })
    }
  }

  return (
    <div style={{ maxWidth: 1200, margin: '0 auto', padding: '0 16px 60px' }}>
      <header style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', padding: '14px 0', borderBottom: '2px solid #4a2040', marginBottom: 20 }}>
        <div style={{ display: 'flex', alignItems: 'center', gap: 14 }}>
          <Link to="/" style={{ display: 'flex', alignItems: 'center', gap: 8, textDecoration: 'none' }}>
            <img src="/gears.svg" alt="synt" style={{ width: 28, height: 28 }} />
            <span className="text-glow" style={{ fontSize: 20 }}>Y.A.R.V</span>
          </Link>
          <nav style={{ display: 'flex', gap: 14, marginLeft: 10 }}>
            <NavLink to="/" end style={({ isActive }) => ({ color: isActive ? '#C1A100' : '#a020f0', textDecoration: 'none', fontSize: 15 })}>Vaults</NavLink>
            <NavLink to="/portfolio" style={({ isActive }) => ({ color: isActive ? '#C1A100' : '#a020f0', textDecoration: 'none', fontSize: 15 })}>My Positions</NavLink>
          </nav>
        </div>
        {address ? (
          <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
            <Identicon address={address} size={22} />
            <span style={{ fontSize: 14 }}>{short(address)}</span>
            <CopyButton text={address} />
            <button className="btn-aqua" style={{ padding: '4px 10px', fontSize: 13 }} onClick={disconnect}>disconnect</button>
          </div>
        ) : (
          <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
            <button className="btn-aqua" onClick={connect} disabled={connecting}>
              {connecting ? 'connecting…' : 'connect wallet'}
            </button>
            <button className="btn-aqua" style={{ padding: '4px 10px', fontSize: 13, opacity: 0.55 }}
              title="testnet/dev only — paste an S… secret, signs locally in memory (never stored)"
              onClick={() => setKeyOpen((v) => !v)}>dev key</button>
          </div>
        )}
      </header>
      {keyOpen && !address && (
        <div style={{ display: 'flex', gap: 8, padding: '10px 0', borderBottom: '1px solid #4a2040', marginBottom: 20, marginTop: -20 }}>
          <input className="synt-input" type="password" autoComplete="off"
            placeholder="S… — held in memory only, cleared on reload/disconnect"
            value={secret} onChange={(e) => setSecret(e.target.value)}
            onKeyDown={(e) => e.key === 'Enter' && onImportKey()}
            style={{ flex: 1, fontSize: 13 }} />
          <button className="btn-aqua" style={{ fontSize: 13 }} onClick={onImportKey} disabled={!secret.trim()}>import</button>
        </div>
      )}
      {children}
      <footer style={{ marginTop: 60, paddingTop: 16, borderTop: '1px solid #4a2040', color: '#775588', fontSize: 13 }}>
        testnet · CL rebalancing vaults on DeFindex-derived vaults + Aquarius ·{' '}
        <a href="https://aqua.network" target="_blank" rel="noreferrer" style={{ color: '#a020f0' }}>aqua.network</a>
      </footer>
    </div>
  )
}
