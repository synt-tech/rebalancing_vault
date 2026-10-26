# Fork-test pipeline (mainnet snapshot)

`test_fork::fork_real_pool_full_flow` runs `cl_aquarius` against a snapshot of
the **real mainnet contracts**: the Aquarius XLM/USDC concentrated pool, the
two Reflector feeds, the XLM/AQUA conversion pool, and the three SACs.

## Regenerating the snapshot

```bash
stellar snapshot create \
  --archive-url https://history.stellar.org/prd/core-live/core_live_001/ \
  --address CBBMQBNHB2FYVZYV7VNHOJHUMTFJLR4PUMRVQYNW6RHIKZO2NQMIBUCV \  # XLM/USDC CL pool
  --address CALI2BYU2JE6WVRUFYTS6MSBNEHGJ35P4AVCZYF3B6QOE3QKOB2PLE6M \  # Reflector DEX feed (AQUA)
  --address CAFJZQWSED6YAWZU3GWRTOCNPPCGBN32L7QV43XX5LZLFTK6JLN34DLN \  # Reflector ext feed (XLM/USDC)
  --address CAVB532FDYEWBXNSCSKACTZI3BV3QBNFLH2UQRR5HIKFKLFVQW5YGZVY \  # XLM/AQUA pool
  --address CDLZFC3SYJYDZT7K67VZ75HPJVIEUVNIXF47ZG2FB2RMQQVU2HHGCYSC \  # XLM SAC
  --address CCW67TSZV3SSS2HXMBQ5JFGCKJNXKZM7UQUWUZPUTHXSTZLEO7SJMI75 \  # USDC SAC
  --address CAUIKL3IYGMERDRUN6YSCLWVAKIFG5Q4YJHUKM4S4NJZQIA3BAS6OJPK \  # AQUA SAC
  --out testing/fork/mainnet_pool.json

# Fetch the deployed WASMs (snapshot omits contract_code):
stellar contract fetch --contract-id <ADDR> \
  --rpc-url https://mainnet.sorobanrpc.com \
  --network-passphrase "Public Global Stellar Network ; September 2015" \
  --out-file <name>.wasm

# Convert to the soroban-ledger-snapshot schema + inject code:
python3 convert.py mainnet_pool.json mainnet_pool_sdk.json \
  <reflector_hash>:reflector_ext.wasm <pool_hash>:aqua_pool.wasm

cargo test -p cl_aquarius_strategy -- --ignored test_fork --nocapture
```

## Status: blocked on local host protocol (2026-10-26)

- Snapshot creation + conversion **works** (59k entries, both WASMs hash-verified
  against on-chain instance executables).
- `Env::from_ledger_snapshot_file` loads fine after pinning
  `protocol_version` to ≤22 in the converted file.
- **Execution blocked**: the deployed pool WASM is interface protocol **v25**
  and the Reflector WASM **v28**; the workspace pins `soroban-sdk 22`
  (env-host rejects "contract protocol number is newer than host"). Running
  the real contracts locally requires upgrading the workspace to SDK ≥25.

## What the fork work already verified (findings)

1. **Oracle coverage split** — no single public Reflector feed covers
   XLM+USDC+AQUA. `CAFJZQW…` serves `Other("XLM")`/`Other("USDC")` ($0.1673 /
   $1.0011 observed); `CALI2BYU…` serves `Stellar(AQUA_sac)` (2.838e10 1e14-scaled,
   USDC base). Fixed by per-asset `OracleRoute` routing (commit 4bb78ad).
2. **Inverted reference-price convention** — `ref_price_1e14` computed the
   USD-ratio `p1/p0` while `check_oracle_band` compares against pool spot
   (units t1/t0); would have reverted on the real pool. Fixed in 4bb78ad.
3. **USDC SAC address typo** — real is `…SJMI75`, not `…SJMIYT`.
4. **Reflector `PriceData.timestamp` is seconds** — `max_oracle_age` safe.
5. **Real pool ABI verified** via `stellar contract info interface`:
   `get_slot0 → Slot0{sqrt_price_x96: U256, tick: i32}`, `deposit_position`,
   `withdraw_position`, `swap`, `swap_strict_receive`, `claim(user)→u128`,
   `get_user_reward(user)→u128`, `get_tick_spacing→i32` — all match the
   hand-written `ClPool`/`Reflector` clients 1:1.
6. **Reflector storage uses timestamp keys** (`{"u64": <ts>}`) — and SACs use
   executable `token` (built-in host code, no WASM fetch needed).

## Pending

- SDK ≥25 upgrade → then run this test for real execution.
- Live simulation probes against mainnet RPC remain available meanwhile
  (`stellar contract invoke` on read-only methods).
