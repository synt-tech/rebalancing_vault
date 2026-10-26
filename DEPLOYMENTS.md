# Testnet deployment (smoke environment)

All addresses on Stellar testnet, deployed by `agent-1`
(`GCAZ36AEUCDY2CS2UTGGCHWEBVJ2ZYGOGOXPQ2GSIH32LM7BTNPW47HY`). Mocks stand in for
Aquarius/Reflector so the full platform wiring is exercised on a real network;
swap into a real testnet CL pool is a later step.

## Platform contracts

| Contract | Address | WASM hash |
|---|---|---|
| DeFindex factory (our instance) | `CDJ467CMUO6R6BLW4PTSMVEGVTUKVNNSIBFLHMK5Q6GK2WMYEDQTKRTR` | `446f6df3…` |
| strategy_registry | `CB4LGZVUF345GF5MLMWIZZR2E222SX6DAPQ2DOP3WEKQKBGJRO7I3RES` | `efbf1ff0…` |
| platform wrapper | `CB74CKXGUTTFP5P3ZEOX3VDR376YJKNLFQ6RKPX6QTEITHNG62D5F6RV` | `8a83f606…` |
| null_router | `CDEYX7WBCCSYXOFBMBOCB2WTLLROPQAWS5YQKUTYSDNBPD23EGASH5LQ` | `c0a49523…` |
| patched vault template | — | `c749c49c…` (installed, deployed per-vault by factory) |
| cl_aquarius template | — | `ea0f490b…` (installed; `21e4cbfd…`/`b0b3b794…` superseded) |

> **Stale as of SDK 29 migration**: all local WASMs rebuilt under
> soroban-sdk 29.0.1 — cl_aquarius source now produces `522457f9…`,
> registry `29b2d56a…`, platform `39abcc38…`, null_router `0738f3c4…`,
> test_mocks `31df971f…`. The addresses above still run the SDK-22
> binaries; redeploy before further testnet work.

Factory init: admin = agent-1, defindex_receiver = agent-1, defindex_fee = 0.

## Current vault (v3, created via `platform.create_vault`)

| Role | Address |
|---|---|
| Vault (clUSDC3) | `CBOOPM4WFKSCXRDYGT5THN3JRKUABUW2VGKURAVDJGCF2GJ2TXU45I6G` |
| Bound cl_aquarius instance | `CBBG2JYTVJPJO3ZKZZID76GX5VNAMTD4NXNHYHCFI4VBTUUIGRLPD6HE` |
| EmergencyManager | platform contract (`CB74CKXG…`) — pinned by wrapper |
| Manager / FeeReceiver / RebalanceManager | agent-1 (operator role) |
| vault_fee | 0 (pinned) · upgradable=false · router=null_router |

Policy (within registered ceilings): perf_fee 1500bps, platform_split 5000bps,
oracle_band 500bps, max_oracle_age 600s, max_slippage 300bps,
max_loss 200bps/3600s window, cooldown 120s, reward floor 0.1 AQUA, reward max age 1d.

## Test fixtures (mocks — NOT real Aquarius/Reflector)

| Fixture | Address | Notes |
|---|---|---|
| Mock XLM/USDC CL pool | `CCUDORIMXIIJYOYYE5NEFBZ25JSJXNMQDPELGQRRHJY4IYNKBWV7AR5S` | test_mocks WASM `4dc6f5ca…`, spot √2.5·2⁹⁶ |
| Mock Reflector oracle | `CBJDZPVX6NI5FWFYV7VILKBKBP7V77II47ATS77TBSXWIKQYWNA56QTU` | prices 1e14-scale: USDC 1.0, XLM 0.4, AQUA 0.2 |
| Mock AQUA/XLM reward pool | `CB5NI7ZGBCSSPUF5635AHJRAZGCMK2YQI7MNT2GBGGJU4XXHP77A6SJN` | |
| USDC SAC | `CBMUABTPC5WH5IRYEQVYTSJ6DSN5SQUK6ROHEXPNDU36V2HHDMRAW7HN` | issuer agent-1 (mintable) |
| XLM SAC | `CBGYSLZB6NQEWQSDCSYGRPWQQHN7DFYNGRO3CC4KVCTIQWMHLJIFMLFA` | |
| AQUA SAC | `CAI4CUGFODGRBM65NCJIL3A2VTTR65AETUGVPU5WTF6XVISQ2MJ5C25Q` | |
| Batch router | `CBS4PBYTM5O26PFSKYGKN4G6ZFRYWKNJKKQ6CL4JFGFNIXHM637CVXZW` | synt_dex router WASM `a0d74427…` — wraps N invokeContract calls into one `exec(caller, calls)` host-function op so single-op wallets (Freighter) can sign zap bundles |

## Verified flows (tx hashes on testnet)

v1 vault (`CCOAAMXR…`/`CCYKUAVO…`, template 21e4cbfd — stale SwapPlan shape):
- `platform.create_vault` → deploy strategy + create vault + bind — `49f782e4…`
- `vault.deposit` 10e9 USDC → 9999999000 shares (1000 dead-share lock) — `f805517d…`
- `vault.rebalance` Invest 8e9 → strategy pull — `a0b74be7…`
- `vault.withdraw` all shares → P2 unwind path delivers 9999999000 — `efcc60b9…`

v3 vault (`CBOOPM4W…`/`CBBG2JYT…`, template ea0f490b — auth fix):
- `platform.create_vault` — `c0cb28b4…`
- deposit → invest → **harvest with live position deploy** (SwapPlan XDR, ticks
  [-100,100], `authorize_as_current_contract` covering pool token pulls) — `0ce3c9a8…`
- `vault.withdraw` all shares through the open position → 9999999000 delivered — `d3a1e313…`
- 2026-10-26: `harvest` → live position deployed at **ticks [-1500, +1500]**
  (±16% band, half=1500 ≤ range_max 2000) — `21e6a5c0…`; deposit of
  82.45 XLM + 7 USDC. CLI arg notes: `harvest`'s `data` (Option<Bytes>) takes
  JSON `{"bytes":"<ScVal hex>"}`; SwapPlan struct-map keys must be XDR-sorted
  (`expiry, new_range, ref_tick, steps`).
- Demo liquidity for the pool chart (mock has no position enumeration —
  frontend reads `KNOWN_POOL_POSITIONS` in `web_frontend/src/config.js`):
  agent-1 `deposit_position` bands [-800,-300] 40 XLM `ad3bd6bd…`,
  [200,700] 25 USDC `e02bac26…`, [-2400,-1600] 30 XLM `abefe8f5…`,
  [1200,2200] 15 USDC `86c17402…`, [-300,300] 20 XLM+8 USDC `0e716784…`.
- **reward conversion** — `set_user_reward` 2e9 AQUA → harvest claimed + swapped via
  reward pool → 970e6 XLM out (oracle min-out 0.97×fair) → `FeeAccrued` event shows
  15% skim split 72.75e6 platform / 72.75e6 operator — `6bd44589…`
- 2026-10-26: **web UI zap flows through the batch router** (one host-function op
  each, Freighter-signed): withdraw 15 shares → 50% USDC / 50%→XLM split —
  `cd4cd8ba…`; 0.5 XLM → swap → deposit zap — `560b7d81…`; withdraw 2 shares via
  in-page dev-key signer — `5a398220…`. All `successful`, `operation_count=1`.

## Notes for real-pool integration

- Strategy init args order: `[pool, oracle, registry, treasury, policy, reward_token, reward_pool]`,
  packed as constructor args `[asset, vec(init_args)]`; optional `init_args[7]`
  = `Vec<OracleRoute>` for per-asset feeds (required on mainnet — XLM/USDC use
  the external Reflector feed via `Other` symbols, AQUA the DEX feed via `Stellar`).
- Policy contracttype map keys must be **sorted** in XDR (`cooldown_secs` first … `tick_cap_bps` last).
- `deploy_strategy` checks `init_args[4]` = Policy against registry ceilings + venue allowlist.
- **Nested-pull auth**: pools pull via `token.transfer(strategy→pool)`, one level
  deeper than invoker auth covers. The strategy must `authorize_as_current_contract`
  before `deposit_position`/`swap` (added in `authorize_pull`) — same pattern the
  DeFindex vault uses for strategy deposits. `mock_all_auths` hides this locally;
  only real-network calls surface it.
- Oracle prices age past `max_oracle_age` quickly on testnet — re-seed `set_price`
  before harvest/withdraw runs.
