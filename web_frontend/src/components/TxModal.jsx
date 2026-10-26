import { CheckCircle, XCircle, ExternalLink } from 'lucide-react'
import Popup from './Popup'
import LoadingGears from './LoadingGears'
import { useStore } from '../store'
import { expertTxUrl } from '../config'

const LABELS = {
  simulating: 'simulating…',
  signing: 'sign in wallet…',
  submitting: 'submitting…',
  pending: 'confirming on-chain…',
}

export default function TxModal() {
  const tx = useStore((s) => s.tx)
  const setTx = useStore((s) => s.setTx)
  if (!tx) return null
  const done = tx.status === 'success' || tx.status === 'error'
  return (
    <Popup onClose={done ? () => setTx(null) : undefined}>
      <div style={{ padding: 20, textAlign: 'center', minWidth: 300 }}>
        <h3 className="text-glow" style={{ marginTop: 0 }}>{tx.title || 'Transaction'}</h3>
        {!done && (
          <div style={{ display: 'flex', flexDirection: 'column', alignItems: 'center', gap: 12 }}>
            <LoadingGears />
            <div>{LABELS[tx.status] || tx.status}</div>
            {tx.hash && <a href={expertTxUrl(tx.hash)} target="_blank" rel="noreferrer" style={{ color: '#a020f0', fontSize: 13 }}>view on stellar.expert</a>}
          </div>
        )}
        {tx.status === 'success' && (
          <div>
            <CheckCircle size={40} color="#00ff64" />
            <div style={{ marginTop: 12 }}>{tx.message || 'success'}</div>
            {tx.hash && <a href={expertTxUrl(tx.hash)} target="_blank" rel="noreferrer" className="btn-aqua" style={{ display: 'inline-block', marginTop: 12, textDecoration: 'none' }}>tx on explorer <ExternalLink size={12} style={{ display: 'inline' }} /></a>}
          </div>
        )}
        {tx.status === 'error' && (
          <div>
            <XCircle size={40} color="#ff4444" />
            <div style={{ marginTop: 12, color: '#ff8888', fontSize: 14, wordBreak: 'break-word' }}>{tx.error}</div>
            <button className="btn-aqua" onClick={() => setTx(null)} style={{ marginTop: 16 }}>close</button>
          </div>
        )}
      </div>
    </Popup>
  )
}
