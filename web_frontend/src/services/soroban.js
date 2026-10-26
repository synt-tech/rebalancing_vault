// Lean Soroban pipeline for the vault UI: simulate (read) + build/sign/send/poll (write).
// Modeled on our prior txSubmit implementation — never blind-retry, poll to final fate.

import {
  rpc, Contract, TransactionBuilder, Networks, BASE_FEE,
  Account, xdr, Address, nativeToScVal, scValToNative,
} from '@stellar/stellar-sdk'
import { RPC_URL, NETWORK_PASSPHRASE, HORIZON_URL, BATCH_ROUTER } from '../config'
import { signTxXdr } from './walletApi'

export const server = new rpc.Server(RPC_URL)

// ---- read path: simulate a contract call, no tx needed ----
const SIM_ACCOUNT = 'GAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAWHF'

export async function simulateView(contractId, method, args = []) {
  const c = new Contract(contractId)
  const account = new Account(SIM_ACCOUNT, '0')
  const tx = new TransactionBuilder(account, {
    fee: BASE_FEE,
    networkPassphrase: NETWORK_PASSPHRASE,
  }).addOperation(c.call(method, ...args)).setTimeout(30).build()
  const sim = await server.simulateTransaction(tx)
  if (rpc.Api.isSimulationError(sim)) throw new Error(sim.error)
  if (sim.result?.retval === undefined) return null
  return scValToNative(sim.result.retval)
}

// ---- contract data read ----
// Instance storage lives inside the contract instance ledger entry as an ScMap
// (not separate ContractDataEntry rows). Fetch the instance entry once and look
// up the DataKey ScVal inside its storage map.
export async function readStorage(contractId, keyScVal) {
  const instKey = xdr.LedgerKey.contractData(
    new xdr.LedgerKeyContractData({
      contract: new Address(contractId).toScAddress(),
      key: xdr.ScVal.scvLedgerKeyContractInstance(),
      durability: xdr.ContractDataDurability.persistent(),
    }),
  )
  const res = await server.getLedgerEntries(instKey)
  const entry = res.entries?.[0]
  if (!entry) return null
  const storage = entry.val.contractData().val().instance().storage()
  if (!storage) return null
  const want = keyScVal.toXDR('base64')
  for (const en of storage) {
    if (en.key().toXDR('base64') === want) return scValToNative(en.val())
  }
  return null
}

export const symKey = (s) => xdr.ScVal.scvVec([xdr.ScVal.scvSymbol(s)])
export const u32Key = (i) => xdr.ScVal.scvVec([xdr.ScVal.scvU32(i)])
export { xdr, Address, Contract }

// debug: raw entry inspection for storage-key work
export async function dumpEntry(contractId, keyScVal) {
  const key = xdr.LedgerKey.contractData(
    new xdr.LedgerKeyContractData({
      contract: new Address(contractId).toScAddress(),
      key: keyScVal,
      durability: xdr.ContractDataDurability.persistent(),
    }),
  )
  const res = await server.getLedgerEntries(key)
  const e = res.entries?.[0]
  if (!e) return { found: false }
  const v = e.val.contractData().val()
  return { found: true, n: res.entries.length, valType: v.switch().name, native: scValToNative(v) }
}

// Wrap a multi-op bundle in the batch router's exec(caller, calls): one
// invokeHostFunction op that runs every sub-call atomically inside. Needed
// because Freighter's sign path only accepts single-op Soroban txs.
const batchExecOp = (from, ops) => {
  const calls = ops.map((op) => {
    const hf = op.body?.().invokeHostFunctionOp?.().hostFunction?.() // xdr.Operation path
    const func = hf ?? (op.type === 'invokeHostFunction' ? op.func : null)
    const ic = func?.invokeContract?.()
    if (!ic) throw new Error('batch exec: only invokeContract ops are supported')
    return xdr.ScVal.scvVec([
      xdr.ScVal.scvAddress(ic.contractAddress()),
      xdr.ScVal.scvSymbol(ic.functionName()),
      xdr.ScVal.scvVec(ic.args()),
      xdr.ScVal.scvBool(false),
    ])
  })
  return new Contract(BATCH_ROUTER).call('exec', new Address(from).toScVal(), xdr.ScVal.scvVec(calls))
}

// ---- write path: build → sign via wallet → send → poll ----
export async function submitOps({ from, ops, onStatus }) {
  onStatus?.('simulating')
  const account = await server.getAccount(from)
  const builder = new TransactionBuilder(account, {
    fee: BASE_FEE,
    networkPassphrase: NETWORK_PASSPHRASE,
  }).setTimeout(60)
  for (const op of ops.length > 1 ? [batchExecOp(from, ops)] : ops) builder.addOperation(op)
  const built = builder.build()

  const sim = await server.simulateTransaction(built)
  if (rpc.Api.isSimulationError(sim)) throw new Error(sim.error)
  const prepared = rpc.assembleTransaction(built, sim).build()

  onStatus?.('signing')
  const signedXdr = await signTxXdr(prepared.toXDR(), from)
  const signed = TransactionBuilder.fromXDR(signedXdr, NETWORK_PASSPHRASE)

  onStatus?.('submitting')
  const send = await server.sendTransaction(signed)
  if (send.status === 'ERROR') throw new Error(`submit rejected: ${JSON.stringify(send.errorResult)}`)
  const hash = send.hash

  onStatus?.('pending')
  // Poll Horizon rather than RPC getTransaction: SDK 13 can't parse the
  // newer transaction-meta union ("Bad union switch: 4") on testnet.
  for (let i = 0; i < 40; i++) {
    const r = await fetch(`${HORIZON_URL}/transactions/${hash}`)
    if (r.status === 200) {
      const j = await r.json()
      if (j.successful) return { hash, result: j }
      const err = new Error('tx failed on-chain')
      err.txResult = j
      throw err
    }
    await new Promise((r2) => setTimeout(r2, 2000))
  }
  throw new Error(`tx ${hash} not confirmed after 80s — check stellar.expert before retrying`)
}

export function submitContractCall({ from, contractId, method, args = [], onStatus }) {
  const op = new Contract(contractId).call(method, ...args)
  return submitOps({ from, ops: [op], onStatus })
}
