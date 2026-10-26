// Deployment configuration — testnet smoke environment (DEPLOYMENTS.md).
// Pool/oracle/reward are MOCKS standing in for Aquarius/Reflector.

export const NETWORK_PASSPHRASE = 'Test SDF Network ; September 2015'
export const RPC_URL = 'https://soroban-testnet.stellar.org'
export const HORIZON_URL = 'https://horizon-testnet.stellar.org'
export const EXPERT_URL = 'https://stellar.expert/explorer/testnet'
export const expertTxUrl = (h) => `${EXPERT_URL}/tx/${h}`
export const expertContractUrl = (c) => `${EXPERT_URL}/contract/${c}`

// Platform contracts
export const FACTORY = 'CDJ467CMUO6R6BLW4PTSMVEGVTUKVNNSIBFLHMK5Q6GK2WMYEDQTKRTR'
export const REGISTRY = 'CB4LGZVUF345GF5MLMWIZZR2E222SX6DAPQ2DOP3WEKQKBGJRO7I3RES'
export const PLATFORM = 'CDKAKRSZZB2NTOBSTT2KDEZSSLOZSQ4DKTGP6RGSJXFUUY6ZQ2HZLPGF'

// Live vault (v4, clUSDC4 — SDK-29 template 522457f9, corrected
// reference-price convention; the mock pool spot is seeded at √0.4·2⁹⁶
// so the older v3 vault's inverted-convention code is no longer usable
// on this shared mock).
export const VAULT = {
  address: 'CB77DI4XYWTQTHS5FJWXFFDG5JQJFARP5KKEDM6GGAJ357XC5ZLBFX3V',
  strategy: 'CABMCEQGTCR7CVEUCQFHP3MFNMLWYH3WUTERVEAKPEHYNOP6U65GFU76',
  name: 'clUSDC4',
  depositAsset: 'USDC',
  pair: 'XLM / USDC',
  template: 'cl_band',
  venue: 'Aquarius',
}

// Fixtures (mocks)
export const POOL = 'CCUDORIMXIIJYOYYE5NEFBZ25JSJXNMQDPELGQRRHJY4IYNKBWV7AR5S'

// Real pool positions deposited by agent-1 on the testnet mock pool
// (the mock exposes no position enumeration, so the liquidity histogram is
// built from this list + the live strategy position).
export const KNOWN_POOL_POSITIONS = [
  { tick_lower: -2400, tick_upper: -1600, liquidity: 4200000000 },
  { tick_lower: -800, tick_upper: -300, liquidity: 5500000000 },
  { tick_lower: -300, tick_upper: 300, liquidity: 6800000000 },
  { tick_lower: 200, tick_upper: 700, liquidity: 3800000000 },
  { tick_lower: 1200, tick_upper: 2200, liquidity: 2600000000 },
]
export const USDC_SAC = 'CBMUABTPC5WH5IRYEQVYTSJ6DSN5SQUK6ROHEXPNDU36V2HHDMRAW7HN'
export const XLM_SAC = 'CBGYSLZB6NQEWQSDCSYGRPWQQHN7DFYNGRO3CC4KVCTIQWMHLJIFMLFA'
export const AQUA_SAC = 'CAI4CUGFODGRBM65NCJIL3A2VTTR65AETUGVPU5WTF6XVISQ2MJ5C25Q'

// Batch router (same WASM as mainnet CCM23MFA…) — wraps several
// invokeContract calls into ONE host-function op so wallets that only sign
// single-op Soroban txs (Freighter) can execute zap bundles atomically.
export const BATCH_ROUTER = 'CBS4PBYTM5O26PFSKYGKN4G6ZFRYWKNJKKQ6CL4JFGFNIXHM637CVXZW'

export const VAULTS = [VAULT]
