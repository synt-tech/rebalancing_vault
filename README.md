# hackmeridian_rebalancing_vault

Rebalancing vaults for Stellar (Soroban) concentrated-liquidity pools.
HackMeridian project workspace — this file is the living project doc;
framing, decisions and TODOs grow here.

## The problem

The concentrated-liquidity (CL) pools now live on Stellar — Aquarius's
`pool_type: "concentrated"` pools and Sushi V3 — behave more like
**orderbooks** than like the constant-product AMMs that came before:

- An LP position is a *range order*: it earns fees and rewards only while
  the pool price sits inside its `[tick_lower, tick_upper)` band. A classic
  x·y=k LP is passive forever; a CL position is a bet on a price interval.
- When the market price exits the band, the position stops earning entirely
  **and** ends up 100% in whichever token just lost — the discrete
  equivalent of a filled limit order.
- As price keeps shifting, somebody has to keep *moving the order*: burn
  the position, rebalance inventory back to the target ratio, re-mint at a
  new band around the new price. On Aquarius today this is a manual
  withdraw → swap → deposit bundle
  (`rebalance_concentrated_positions_wrapper.py`).

The measured cost of doing this badly, from our own account:

- **−719.91 sUSD in one day** (2026-09-26) from an unguarded rebalance swap.
- **+1,176 sUSD** fees earned vs **−918 sUSD** value destroyed over the whole
  post-Sep-26 manual run (net +258).
- Median execution leak ~3–3.5 sUSD/tx → a position must earn **more than
  that in fees between triggers** or rebalancing is negative-EV.

So the vault productizes order management: contract custody + policy
on-chain, strategy off-chain, atomic rebalances.

## The goal: a pooled share-token vault on Soroban

The hackathon project builds and deploys a **public share-token vault**
for Aquarius concentrated pools — the "deposit-and-forget" LP pattern that
made concentrated liquidity usable for ordinary users on every chain where
it shipped:

- A user deposits tokens (eventually single-sided), and receives a
  fungible **vault-share token** — a pro-rata claim on the managed
  position plus accrued fees and rewards. No ranges to pick, no bundles to
  sign, no trigger config.
- The contract owns the CL position(s); an off-chain keeper rebalances
  inside on-chain policy bounds; swap fees and AQUA rewards compound back
  into the share price.
- One deposit tx in, one withdraw tx out — the vault absorbs the
  order-management complexity CL introduced.

This is the pattern that won wherever it shipped:

- **Gamma Hypervisor** (Uniswap V3 / Algebra): the archetype — dual
  base+limit positions behind fungible shares, deployed as the built-in
  "automated liquidity" option inside QuickSwap, Camelot, Saucerswap.
- **Kamino** (Solana, Orca/Raydium/Meteora): the dominant LP product on
  Solana — kToken shares, auto-rebalance, auto-compound, single-sided
  deposits.
- **Beefy CLM**: whitelisted-keeper variant with calm-period TWAP guards.
- **Charm Alpha Vaults**: the original (2021) — share token over a
  full-range + base + limit 3-position structure.
- **DeFindex**: the only Stellar-native vault protocol — SEP-41 share
  vaults with pluggable strategies, but **no CL strategy exists today**;
  that gap is the hackathon opportunity.

### Don't reinvent: host it as a DeFindex strategy

Rather than building our own share-token vault, the plan is to write the
CL position manager as a **DeFindex strategy** and let their audited
vault do the pooled-custody work (local clone: `defindex/`):

- **What a strategy is** (`apps/contracts/strategies/core/src/lib.rs`):
  six methods — `deposit`/`withdraw`/`balance` in underlying-asset terms,
  `harvest(from, data)` as the permissioned active-action hook ("claiming
  and compounding rewards, rebalancing a drifting position, rolling an
  expiring one" — their docs literally describe our rebalance). `data` is
  an opaque blob conventionally a min-out.
- **Mapping for CL**: single-asset **USDC** vault holding one
  `cl_aquarius` strategy. The strategy internally owns the CL position:
  deposit swaps/pairs USDC→(token0,USDC)→`deposit_position`; `balance()`
  reports position value in USDC at the pool's own `sqrt_price_x96`
  (their rule: underlying-asset value at live rate, never shares);
  `harvest` = keeper entry for claim-fees/compound/re-range; `withdraw`
  unwinds pro-rata + swaps back to USDC.
  (Vault asset decided USDC, 2026-10-25 — earlier drafts said sUSD;
  `cl_vault_valuation.md` is the `balance()` design doc.)
- **What we get free**: dfToken share minting + proportional multi-asset
  share math, fee locking/distribution (`vaultFee` on yield, `defindexFee`
  = their share *of* our fee), Manager/Emergency/Rebalance-manager roles,
  `rescue()`/pause unwind-to-idle, per-strategy performance `report`s,
  factory deployment, public API (vault metrics/APY/user positions/tx
  history) for the frontend — and an **OtterSec-audited** vault
  (`defindex/audits/`). Our audit surface shrinks to just the strategy.
- **What we keep from rocky-vault**: the whole strategy engine —
  keeper+`RebalancePlan` validation (allowlists, hops, expiry, tick
  drift, range width), `fair_in` slippage clamp, share_bps inventory
  band, atomic withdraw→swap→deposit — moves *inside* the strategy
  contract as init_args config + `harvest`-time checks.
- **Caveats to verify**: deposit/withdraw pairing swaps now land
  per-user (mitigate by depositing at the position's current ratio and
  swapping only the deficit); `rescue()` must tolerate a full
  unwind+swap; DeFindex terms/protocol fee for a custom strategy;
  `from.require_auth()` means the vault authorizes every call — auth
  paths differ from rocky-vault's direct-invoker flow and need a
  real-auth test.

Why it matters specifically here: on the other chains the share-vault
absorbed the exact pain we measured on our own account (out-of-range =
zero yield, manual rebalances leaking ~3.5 sUSD/tx). Stellar has CL pools
(Aquarius, Sushi V3) but no share-vault product — today the only way to LP
is hand-managing positions.

### Aquarius specifics that shape the design

- Pools: `sqrt_price_x96` (Q64.96), ticks = 1.0001^tick; fee tiers
  `10 / 30 / 100` → tick spacing `20 / 60 / 200`; bounds ±887272.
- Position ops are on the **pool contract**, not the router:
  `deposit_position`, `withdraw_position` (always sweeps accrued fees —
  partial withdraws too), `estimate_*` simulation twins, `get_position*`,
  `get_active_liquidity`, tick bitmap getters.
- Swaps: router `swap` / `swap_chained` (`*_strict_receive` variants).
  Exact-input `swap` does **not** refund unspent remainder — it's treated
  as fee; always `estimate_swap` first and set `out_min`.
- Rewards accrue **only while in range**, and the in-range check is
  checkpoint-based — it re-syncs only when the owner interacts (deposit /
  withdraw / claim). A vault that acts on the position also refreshes the
  reward weight; a stale position keeps accruing at the old weight.
- Events for the keeper's trigger feed: `trade`, `pool_state`
  (sqrt_price_x96, tick, active_liquidity), `position_update`.
- Sushi V3 on Stellar: Uni-V3-style UI (fee tiers 0.05 / 0.3 / 1%); Rust
  pool source is private — verify semantics against the deployed contracts
  and collected `sushi-stellar-sdk` / `sushi-stellar-interface` snapshots,
  not assumptions.

## Design stance (carried over from prior work)

The rocky-vault prototype (extracted at `synt_dex/rocky-rebalancer/`,
`contracts/rocky-vault/`) is **deployed and exercised on testnet**
(2026-10-02): vault `CBHV4KFGKKYJPS6CB65JCPERSWPTEO4AZJ5A7UYLAUJMTU2L2NELT4GA`
managing an XLM/AQUA CL position — opened, then rebalanced through the 1%
route pool to a 3055 bps token1 share. A first attempt through the 0.3%
pool correctly reverted: its price was ~38% off the CL reference, so the
required input exceeded the fair-price bound — the clamp caught a bad
route live. Testnet keys exist in the stellar CLI: `admin`, `rocky-keeper`.

- **Private contract-owned position**; `admin` (funds/policy) + `keeper`
  (submits `RebalancePlan`) — Gamma's `onlyOwner` + Revert's
  permissioned-executor hybrid.
- **Keeper plan fully re-validated on-chain**: pool/token allowlists, hop
  count, expiry, cooldown, `max_tick_delta` calm window, tick-spacing
  alignment, range-width bounds, `share_bps` inventory band.
- **Slippage bound from the managed pool's own price**:
  `effective_max_in = min(plan.max_in, fair_in × (1 + max_slippage_bps))`
  — the pool IS the oracle; the Sep-26 incident is what a missing bound
  costs.
- **Atomic rebalance in one tx**: withdraw → recompute exact `out_amount`
  → `swap_chained_strict_receive` → redeposit → verify band + tolerance +
  loss cap.
- **Trigger = range exit.** The 27-pool sweep found "rebalance at range
  exit" optimal on every pool measured; early triggers paid the swap leak
  2–3× more often for ≤ ~5% less divergence. Volatility-scaling belongs in
  the **width**, not the trigger.
- **Prototype was private; the product adds the share layer — via
  DeFindex** — the custody/keeper/validation spine stays as built and
  testnet-proven; the share minting, donation/inflation guards, fee
  accounting, roles and rescue come from their audited vault instead of
  new code. See "Don't reinvent" above.

## Venue adapters

| Venue | Status | Notes |
|---|---|---|
| Aquarius | interface live + documented | prototype existed as rocky-vault (see resources) |
| Sushi V3 | adapter needed | Uni-V3 semantics assumed; verify against deployed WASM |

## Resources already in this workspace

| Path | What it is |
|---|---|
| `rebalancing_vaults_research.md` | code-level survey: Gamma, Arrakis, Revert, Beefy, Charm, Steer, DeFindex — and what rocky-vault took/rejected |
| `cl_vault_landscape.md` | 7-type CL-vault taxonomy, trigger/swap-bound/position-shape/custody comparison tables, cautionary history |
| `stellar/aquarius_dex/` | `aquarius_client.py`, `rebalance_solver.py`, `backtest_rebalance_policy.py`, pool PnL/cost analyzers, `pool_cache/`, live snapshots |
| `stellar/aquarius_dex/research/` | CLMM economics literature: LVR σ²/8, width/trigger sweeps, vol-regime experiments (README maps findings → design) |
| `security/Aquarius-security-review/` | Aquarius `soroban-amm` local working copy (upstream repo private — unmatched-to-deployed reference) |
| `synt_dex/rocky-rebalancer/` | **rocky-vault prototype** — contract (`contracts/rocky-vault/`), tests, testnet deployment notes (README), pinned reference sources under `reference/` (extracted from `synt_tech_website.zip`; gitignored, regenerable) |
| `rebalance_concentrated_positions_wrapper.py` | the manual rebalance flow the vault replaces |
| `../defindex/` | DeFindex local clone — audited vault + strategy trait (`apps/contracts/strategies/core/`) + Blend/hodl/soroswap example strategies; the share-vault infra we plug into |
| `reference/vault_docs/defindex/` | DeFindex docs corpus (76 pages): build-a-strategy guide, roles, fees, API |

## Open questions / TODO

- [ ] ~~Vault product shape~~ → decided: public share-token vault hosted
      as a **DeFindex strategy** (see "Don't reinvent"). TODO: write
      `cl_aquarius` implementing `DeFindexStrategyTrait` — port the
      rocky-vault policy engine inside (plan validation, fair_in clamp,
      share_bps band), internal pairing swaps on deposit/withdraw,
      `balance()` in underlying at pool ref price, keeper `harvest` for
      claim/compound/re-range, rescue-safe full unwind. Verify: DeFindex
      protocol-fee terms for custom strategies, real-auth tests for
      vault→strategy→pool call chains.
- [ ] Sushi adapter — how close to Uni-V3 semantics is the deployed pool?
      rocky-vault's own notes flag NFT positions, oracle hints and a
      different swap ABI as the deltas to resolve; skim `sushi-stellar-*`
      snapshots under `synt_dex/rocky-rebalancer/reference/` and diff
      against Aquarius's documented behavior.
- [ ] Keeper trigger: subscribe `trade`/`pool_state` events, fire only at
      range exit — vs polling loop.
- [ ] Multi-position (base + one-sided limit) to park leftover inventory
      without swapping — Gamma's reason for two positions.
- [ ] Rewards: `claim_rewards` exists in rocky-vault but was never
      exercised on testnet (needs pool gauge config) — claim/re-stake
      inside the vault, or leave to admin sweep? (note the
      checkpoint-stale-weight behavior above)
- [ ] Demo story: live testnet vault vs recorded live-pool replay?

## Links

- Hackathon: https://www.hackmeridian.com/
- Aquarius CL dev docs: https://docs.aqua.network/developers/concentrated-liquidity
  - interface ref: …/contract-interface-reference
- Sushi on Stellar: https://www.sushi.com/stellar/pool
- Stellar pool tracker: https://stellarindex.io/dexes/sushiswap_v3/
