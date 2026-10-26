// Slim stellar config for the vault UI — testnet only for now.
// walletApi.js (reused from an earlier internal project) imports NETWORK_PASSPHRASE/IS_TESTNET_MODE.

import { Networks } from '@stellar/stellar-sdk'

export const IS_TESTNET_MODE = true
export const NETWORK_PASSPHRASE = Networks.TESTNET
export const HORIZON_URL = 'https://horizon-testnet.stellar.org'
export { expertTxUrl } from '../config'
