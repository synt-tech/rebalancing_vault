import { X, AlertCircle, CheckCircle, Info } from 'lucide-react'
import { useStore } from '../store'

const icons = { error: AlertCircle, success: CheckCircle, info: Info }
const colors = { error: '#ff4444', success: '#00ff64', info: '#C1A100' }

export default function ToastContainer() {
  const toasts = useStore((s) => s.toasts)
  const dismiss = useStore((s) => s.dismissToast)
  if (!toasts.length) return null
  return (
    <div style={{ position: 'fixed', bottom: 16, right: 16, zIndex: 100, display: 'flex', flexDirection: 'column', gap: 8, maxWidth: 420 }}>
      {toasts.map((t) => {
        const Icon = icons[t.type] || Info
        return (
          <div key={t.id} className="synt_tech_card" style={{ borderColor: colors[t.type] || '#4a2040', display: 'flex', gap: 8, alignItems: 'flex-start', padding: 10 }}>
            <Icon size={16} color={colors[t.type]} style={{ flexShrink: 0, marginTop: 2 }} />
            <div style={{ flex: 1, fontSize: 14 }}>
              {t.text}
              {t.link && <a href={t.link} target="_blank" rel="noreferrer" style={{ color: '#a020f0', display: 'block', marginTop: 4 }}>view on stellar.expert</a>}
            </div>
            <button onClick={() => dismiss(t.id)} style={{ background: 'none', border: 'none', color: '#888', cursor: 'pointer', padding: 0 }}>
              <X size={14} />
            </button>
          </div>
        )
      })}
    </div>
  )
}
