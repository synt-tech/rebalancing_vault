// Wallet integration using Stellar Wallets Kit
// Supports LOBSTR, Freighter, Albedo, Hana, HOT, xBULL, Rabet, WalletConnect

import { StellarWalletsKit } from '@creit.tech/stellar-wallets-kit/sdk'
import { defaultModules } from '@creit.tech/stellar-wallets-kit/modules/utils'
import { WalletConnectModule, WalletConnectTargetChain } from '@creit.tech/stellar-wallets-kit/modules/wallet-connect'
import { KitEventType, SwkAppDarkTheme } from '@creit.tech/stellar-wallets-kit/types'
import { Keypair, Transaction } from '@stellar/stellar-sdk'
import { NETWORK_PASSPHRASE, IS_TESTNET_MODE } from './stellarConfig'

const WC_PROJECT_ID = import.meta.env.VITE_WALLETCONNECT_PROJECT_ID || '99488d1457394f34756259d47601d15d'

// Suppress known-benign WalletConnect SDK internal warnings.
// The WC SDK calls Core.init() internally from both SignClient and AppKit,
// producing a "Core is already initialized" warning that cannot be avoided
// from consumer code. We filter only this specific message.
const _origWarn = console.warn
const _origError = console.error
console.warn = function (...args) {
  const msg = typeof args[0] === 'string' ? args[0] : ''
  if (msg.includes('WalletConnect Core is already initialized')) return
  if (msg.includes('Lit is in dev mode')) return
  return _origWarn.apply(console, args)
}
console.error = function (...args) {
  const obj = args[0]
  if (obj && typeof obj === 'object' && obj.level === 50) {
    // Suppress all WC SDK internal level-50 errors:
    // - onRelayMessage() failures (stale session relay messages)
    // - "Restore will override" (WC history/expirer restore)
    // - bare {time, level: 50} objects (companion log to relay errors)
    return
  }
  return _origError.apply(console, args)
}

// Clear stale WalletConnect session data to prevent relay message errors
function clearStaleWCStorage() {
  try {
    const keys = Object.keys(localStorage)
    for (const key of keys) {
      if (key.startsWith('wc@') || key.startsWith('walletconnect') || key.startsWith('reown')) {
        localStorage.removeItem(key)
      }
    }
  } catch (e) {
    // localStorage might not be available
  }
}

let initPromise = null

function ensureInit() {
  if (initPromise) return initPromise
  initPromise = Promise.resolve().then(() => {
    clearStaleWCStorage()
    StellarWalletsKit.init({
      theme: SwkAppDarkTheme,
      modules: [
        ...defaultModules(),
        new WalletConnectModule({
          projectId: WC_PROJECT_ID,
          metadata: {
            name: 'synt_dex',
            description: 'synt_tech_dex — stellar trading interface',
            url: window.location.origin + import.meta.env.BASE_URL,
            icons: [`${window.location.origin}${import.meta.env.BASE_URL}gears.svg`],
          },
          allowedChains: [IS_TESTNET_MODE ? WalletConnectTargetChain.TESTNET : WalletConnectTargetChain.PUBLIC],
        }),
      ],
    })
  })
  return initPromise
}

export async function connectWallet() {
  await ensureInit()
  await StellarWalletsKit.authModal()
}

export async function switchWallet(walletId) {
  await ensureInit()
  await StellarWalletsKit.disconnect()
  StellarWalletsKit.setWallet(walletId)
  await StellarWalletsKit.fetchAddress()
}

export async function disconnectWallet() {
  await ensureInit()
  await StellarWalletsKit.disconnect()
}

export async function getWalletAddress() {
  await ensureInit()
  try {
    const { address } = await StellarWalletsKit.getAddress()
    return address
  } catch {
    return null
  }
}

// ---- In-memory secret key signer ----
// An imported secret key signs transactions locally, without any wallet
// extension. The key is held ONLY in module memory (never localStorage /
// sessionStorage / network) and is cleared on page reload.
let localKeypair = null
let localPublicKey = null

export function importSecretKey(secret) {
  const kp = Keypair.fromSecret(String(secret).trim())
  localKeypair = kp
  localPublicKey = kp.publicKey()
  return localPublicKey
}

export function hasLocalSigner() {
  return localKeypair !== null
}
export function getLocalPublicKey() {
  return localPublicKey
}
export function clearLocalSigner() {
  localKeypair = null
  localPublicKey = null
}

export async function signTxXdr(xdr, address) {
  if (localKeypair && (!address || address === localPublicKey)) {
    const tx = new Transaction(xdr, NETWORK_PASSPHRASE)
    tx.sign(localKeypair)
    return tx.toXDR()
  }
  await ensureInit()
  const { signedTxXdr } = await StellarWalletsKit.signTransaction(xdr, {
    networkPassphrase: NETWORK_PASSPHRASE,
    address,
  })
  return signedTxXdr
}

/**
 * Sign an arbitrary message/payload with the connected wallet.
 * Used by stellar_broker for session authorization payloads.
 * Returns the wallet's signedMessage (format depends on wallet).
 */
export async function signData(message, address) {
  await ensureInit()
  const { signedMessage } = await StellarWalletsKit.signMessage(message, { address })
  return signedMessage
}

export function onWalletStateChange(callback, onWalletSelected) {
  let subs = null
  let cancelled = false
  ensureInit().then(() => {
    if (cancelled) return
    const sub1 = StellarWalletsKit.on(KitEventType.STATE_UPDATED, (event) => {
      callback(event.payload?.address || null)
    })
    const sub2 = StellarWalletsKit.on(KitEventType.DISCONNECT, () => {
      callback(null)
    })
    const sub3 = onWalletSelected
      ? StellarWalletsKit.on(KitEventType.WALLET_SELECTED, (event) => {
          onWalletSelected(event.payload?.id || null)
        })
      : null
    subs = sub3 ? [sub1, sub2, sub3] : [sub1, sub2]
  })
  return () => {
    cancelled = true
    if (subs) {
      subs.forEach(s => s())
    }
  }
}

export function createWalletButton(element, options = {}) {
  ensureInit().then(() => {
    StellarWalletsKit.createButton(element, options)
  })
}
