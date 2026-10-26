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
  --address CAS3J7GYLGXMF6TDJBBYYSE3HQ6BBSMLNUQ34T6TZMYMW2EVH34XOWMA \  # XLM SAC (native)
  --address CCW67TSZV3SSS2HXMBQ5JFGCKJNXKZM7UQUWUZPUTHXSTZLEO7SJMI75 \  # USDC SAC
  --address CAUIKL3IYGMERDRUN6YSCLWVAKIFG5Q4YJHUKM4S4NJZQIA3BAS6OJPK \  # AQUA SAC
  --address CAUJNVN5DWOBCKLV5KPR3KRQBKRI7L2X7VOUAWF5FDISVSQNXCEYITPJ \  # pool ConfigStorage
  --address CCABO2IQYDWRGGQ4DYQ73CV3ZFDBRZTEQNDDJMFT7JZO54CLS4RYJROY \  # pool Plane
  --address CBKCROE56TU2FTT3C5CVN676PYVLTOQUQDHHH57GLWDY5VOKSCZPGOFN \  # RewardBoostFeed (locker_feed)
  --address CARCKZ66U4AI2545NS4RAF47QVEXG3PRRCDA52H4Q3FDRAGSMP4BRU3W \  # RewardBoostToken (lock token)
  --address CBQDHNBFBZYE4MKPWBSJOPIYLW4SFSXAXUTSXJN76GNKYVYPCKWC6QUK \  # Router
  --address CBEU6WHTPJMWWILPO5FJI7YUH6225ZZSX6MIK3MMR4HRRPZVLUIW6HKI \  # VaultAddr
  --address CAHEWHOPRKQYGOTMBMQMLAMNRHS3QUCPMFDZ6PQY4PKU4Q4XHFKAJFF \  # USDC whale (funding source)
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

## Status: PASSING end-to-end (2026-10-26)

`fork_real_pool_full_flow` executes the full lifecycle against real
mainnet state (ledger ~65.14M) with the strategy registered as its
**compiled WASM** (native fallback if the artifact is missing):

```
POOL slot0.tick=-17872 spacing=20 t0=XLM t1=USDC
REFLECTOR_EXT Other(usdc) -> 100110783974489   ($1.0011, 1e14-scaled)
REFLECTOR_EXT Other(xlm)  -> 16776471223071    ($0.1678)
REFLECTOR_EXT Other(aqua) -> None              (coverage split confirmed)
REFLECTOR_DEX Stellar(AQUA) -> 28451810363
deposit -> 10000000                  (USDC whale transfer + band check)
harvest(unwind-only) -> Ok(Ok(()))
redeploy range [-18380,-17880]       (single-sided USDC band below spot)
harvest(redeploy) -> ok              (real deposit_position, positive liq)
balance -> 9999999
withdraw 5000000 -> Ok(Ok(5000000))
```

### Resource profile (InvocationResources, mainnet metered)

vs `tx_max_instructions` = 400M and `tx_memory_limit` = 40 MiB (live
`stellar network settings`, 2026-10-26):

| op          | instructions | % of 400M | mem bytes | write entries |
|-------------|--------------|-----------|-----------|---------------|
| deposit     |        5.3 M |      1.3% |     4.9 M |             3 |
| unwind hvst |       15.5 M |      3.9% |    13.4 M |             3 |
| redeploy    |      156.5 M |     39.1% |    23.6 M |            13 |
| withdraw    |      274.8 M |     68.7% |    25.9 M |            14 |

All ops fit comfortably in a single mainnet transaction. Withdraw is the
heaviest path (~69% of the instruction budget) because it walks the real
pool's `withdraw_position` → tick/liquidity updates → boost working-balance
checkpoint → `Plane.update` → SAC transfers. Keep compound ops
(harvest+withdraw) out of single transactions in the keeper design.

- Workspace is on **soroban-sdk 29.0.1** — env-host accepts the deployed
  WASMs (pool v25, Reflector v28). `defindex-strategy-core` is vendored
  into `contracts/strategy_core` because upstream pins SDK 22 and its
  `Env`/`Address` types don't unify across SDK majors.
- Snapshot creation + conversion works; `Env::from_ledger_snapshot_file`
  loads; real `get_slot0`/`lastprice` calls execute locally.
- Snapshot must include, beyond the contracts themselves:
  - **SAC issuer accounts** (`--address G…`): every SAC balance path calls
    `is_asset_auth_required` → reads the issuer's classic account flags.
    USDC=`GA5ZSEJYB…KZVN`, AQUA=`GBNZILST…AQUA`.
  - **The XLM SAC** `CAS3J7GY…` (native asset's real contract — easy to
    mistype; take it from `pool.get_tokens()`).
  - **The pool's external dependencies**: `deposit_position` internally
    `try_call`s `RewardBoostToken.balance(user)` + `RewardBoostFeed.total_supply()`
    (boost manager) and updates `Plane`. Missing instances escalate to
    `Error(Storage, MissingValue)` → `Abort` — not a strategy failure.
    All six (`ConfigStorage`, `Plane`, `RewardBoostFeed`, `RewardBoostToken`,
    `Router`, `VaultAddr`) are enumerated in the pool's instance storage —
    include them all rather than discovering one at a time.
  - **A funding source**: SAC `mint` needs the issuer account anyway;
    simpler to `transfer` from an existing `Balance` holder (top USDC
    whale `CAHEWHOP…JFF` has ~55M).
  - **WASMs**: archives emit no `contract_code` — `stellar contract fetch`
    each executable and inject via `convert.py hash:file` args.
    SACs are executable `token` (built-in) — no wasm needed.

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
7. **CL-geometry**: for `t0=XLM, t1=USDC`, a range **below** the current
   tick is `token1`-only → that's where a single-sided USDC balance
   deploys (the inverted choice yields liquidity 0 →
   `Contract(2006)`/`OutMinNotSatisfied`). The test derives the band from
   live `slot0.tick`/`tick_spacing`, so it stays valid as price moves.
8. **External-dependency enumeration**: read the pool's *instance storage*
   (`ConfigStorage`, `Plane`, `RewardBoostFeed`, `RewardBoostToken`,
   `Router`, `VaultAddr`) and snapshot all of them — failures surface one
   call at a time as `Error(Storage, MissingValue)` → `Abort`.

## Pending

- None — the pipeline runs end-to-end. Regenerate `mainnet_pool.json`
  before each audit pass; ranges are tick-relative so they track drift.
