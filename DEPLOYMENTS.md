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
- **reward conversion** — `set_user_reward` 2e9 AQUA → harvest claimed + swapped via
  reward pool → 970e6 XLM out (oracle min-out 0.97×fair) → `FeeAccrued` event shows
  15% skim split 72.75e6 platform / 72.75e6 operator — `6bd44589…`

## Notes for real-pool integration

- Strategy init args order: `[pool, oracle, registry, treasury, policy, reward_token, reward_pool]`,
  packed as constructor args `[asset, vec(init_args)]`.
- Policy contracttype map keys must be **sorted** in XDR (`cooldown_secs` first … `tick_cap_bps` last).
- `deploy_strategy` checks `init_args[4]` = Policy against registry ceilings + venue allowlist.
- **Nested-pull auth**: pools pull via `token.transfer(strategy→pool)`, one level
  deeper than invoker auth covers. The strategy must `authorize_as_current_contract`
  before `deposit_position`/`swap` (added in `authorize_pull`) — same pattern the
  DeFindex vault uses for strategy deposits. `mock_all_auths` hides this locally;
  only real-network calls surface it.
- Oracle prices age past `max_oracle_age` quickly on testnet — re-seed `set_price`
  before harvest/withdraw runs.
