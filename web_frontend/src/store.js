import { create } from 'zustand'
import { connectWallet, disconnectWallet, getWalletAddress, hasLocalSigner, getLocalPublicKey, clearLocalSigner } from './services/walletApi'

let toastSeq = 0

export const useStore = create((set, get) => ({
  // ---- wallet ----
  address: null,
  walletId: null,
  connecting: false,
  async connect() {
    // local-secret signer (testnet/demo): skip the wallet-kit modal entirely
    if (hasLocalSigner()) {
      set({ address: getLocalPublicKey() })
      return
    }
    set({ connecting: true })
    try {
      await connectWallet()
      const address = await getWalletAddress()
      set({ address, connecting: false })
    } catch (e) {
      set({ connecting: false })
      if (!/cancel|reject|closed/i.test(e?.message || '')) {
        get().toast({ type: 'error', text: `connect failed: ${e.message}` })
      }
    }
  },
  // dev/test: import a local secret inside the app's own walletApi instance
  async connectLocal(secret) {
    const { importSecretKey } = await import('./services/walletApi')
    set({ address: importSecretKey(secret) })
  },
  async disconnect() {
    clearLocalSigner()
    await disconnectWallet().catch(() => {})
    set({ address: null, walletId: null })
  },

  // ---- toasts ----
  toasts: [],
  toast({ type = 'info', text, link }) {
    const id = ++toastSeq
    set((s) => ({ toasts: [...s.toasts, { id, type, text, link }] }))
    setTimeout(() => set((s) => ({ toasts: s.toasts.filter((t) => t.id !== id) })), 12000)
  },
  dismissToast(id) {
    set((s) => ({ toasts: s.toasts.filter((t) => t.id !== id) }))
  },

  // ---- tx modal ----
  tx: null, // {status, hash, title, error}
  setTx(tx) { set({ tx }) },

  // ---- wallet balances (shared feed) ----
  usdcBalance: null,
  setUsdcBalance: (b) => set({ usdcBalance: b }),
}))

// dev/test hook: drive wallet state + local signer from the console
if (import.meta.env.DEV) window.__store = useStore
