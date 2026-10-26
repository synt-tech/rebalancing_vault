# Y.A.R.V. — yet-to-be-named automated rebalancing vaults

Rebalancing vaults for Stellar (Soroban) concentrated-liquidity pools.
HackMeridian project workspace — this file is the living project doc;
framing, decisions and TODOs grow here.

## The problem

The concentrated-liquidity (CL) pools now live on Stellar — Aquarius's
`pool_type: "concentrated"` pools and Sushi V3 — behave in many ways more 
like **orderbooks** than like the constant-product AMMs that came before:

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

Doing this badly is expensive: an unguarded rebalance swap can lose more
in one transaction than the position earns in days, and every rebalance
leaks execution cost — a position must earn **more than that leak in fees
between triggers** or rebalancing is negative-EV.

So the vault productizes order management: contract custody + policy
on-chain, strategy off-chain, atomic rebalances.

## The goal: a CL rebalancing platform on Soroban

The hackathon project builds and deploys a **self-operated CL rebalancing
platform**: our own vault factory on Soroban hosting **public share-token
vaults** for Aquarius concentrated pools — the "deposit-and-forget" LP
pattern that made concentrated liquidity usable for ordinary users on
every chain where it shipped:

- A user deposits tokens (eventually single-sided), and receives a
  fungible **vault-share token** — a pro-rata claim on the managed
  position plus accrued fees and rewards. No ranges to pick, no bundles to
  sign, no trigger config.
- The contract owns the CL position(s); an off-chain keeper rebalances
  inside on-chain policy bounds; swap fees and AQUA rewards compound back
  into the share price.
- One deposit tx in, one withdraw tx out — the vault absorbs the
  order-management complexity CL introduced.
- **The platform is the product**: the same vault+strategy machinery is
  open to third-party strategists — many sustainable rebalancing
  policies exist (the five families below), and they earn by operating
  a vault on their strategy (the operator leg of the income fee; no
  separate author fee, matching every major CL-vault platform). We run
  the factory and the fee layer; `cl_aquarius` is the first/reference
  strategy.

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

### What strategies actually run in these vaults

Surveyed in `../cl_rebalance_strategies.md` — production vaults run five
policy families:

| Family | Who runs it | Policy |
|---|---|---|
| Re-center on exit | Kamino (8 on-chain rebalance methods incl. Tracker/Periodic/Drift), Revert AutoRange, Beefy CLM, Gamma | fixed width; on range exit/timer → swap to ratio, re-mint centered on new price |
| Inventory-ratio target | ICHI YieldIQ, Algebra A-ALM, **`cl_band`** | hold an asset-ratio band; rebalance when it drifts (ICHI/Algebra reshape ranges *without* swapping; `cl_band` swaps the deficit) |
| Peg/median-anchored | ICHI Ascend/Flip, Gamma pegged presets | stable/correlated pairs: center on a median-of-N-observations peg; tiny bounded swaps |
| Indicator-driven | Steer (WASM marketplace: Keltner/Bollinger/MA), Sommelier | off-chain compute emits range plans; width scaled by volatility indicators |
| No-swap reuse / pool-native | Beefy main+alt, Gamma base+limit, Maverick bins, Bunni LDF | carry leftover inventory one-sided or let the pool move liquidity — never sell to rebalance |

`cl_band` sits in the **inventory-ratio family** (the ICHI/Algebra
lineage): a `share_bps` band `[lower,upper]` → swap residual to `target`
± tolerance. Its on-chain `Policy` is the strictest of the set — 13
fields (band, slippage+loss caps, route/tick/width bounds, cooldown,
allowlists) vs Charm's 6-param `PassiveStrategy` or Gamma's
trust-the-owner — because it assumes a semi-trusted keeper and,
post-DeFindex, public depositors. Real gaps vs production: no
no-swap leftover handling (single position; base+limit is TODO'd) and no
volatility-scaled width (Steer/ICHI are the blueprints). Everything here
remains novel surface on Stellar — DeFindex's own strategies are lending
autocompounds, so nothing at this layer exists to copy or conflict with.

### Don't reinvent: build the platform on DeFindex's audited code

Rather than writing share-vault custody/share/fee machinery from scratch,
the platform runs on **DeFindex's audited vault + factory** (local
clone: `defindex/`) — strategies first, no platform fork
(`../platform_design.md` §0):

- **Our own factory instance** (stock code) — we are its admin. Zero
  diff. Its protocol-fee slot is inert (`vault_fee = 0`); the platform
  is paid from the strategy-side income fee.
- **CL logic lives in strategies** — `cl_aquarius` implements the
  unchanged `DeFindexStrategyTrait`; its `harvest` authorizes the
  vault's RebalanceManager via the public `get_rebalance_manager()`
  getter, so keepers use vault roles without a vault change.
- **Our `strategy_registry` + `platform` contracts** beside the vault —
  strategies deploy only from reviewed template hashes, venues
  (pools/routers) come from a curated list, each instance binds to one
  vault, and listed vaults are created non-upgradable with
  `vault_fee = 0` — the only fee is the strategy's cut of collected
  income (LP fees + rewards), accrued in the strategy and pulled by the
  platform/operator.
  The vault's generic Soroswap swap path is neutralized with a
  reverting router address.
- **A two-item vault patch, for CL correctness** — (P1) the stock
  `invest_in_strategy` overwrites a strategy's carried loss, so the
  recovery gets fee-locked as profit (reproduced in a unit test);
  (P2) withdrawals check `min_amounts_out` against the requested, not
  the delivered, amount — fatal for a vault whose exits need a swap.
- **Phase 2**: third-party strategists run their own templates as
  operators (earning the operator leg of the strategy income fee — no
  separate author fee, matching the category) behind a manager proxy
  that blocks `vault_fee` and timelocks fee increases.

What we inherit untouched: dfToken share minting + proportional
multi-asset share math, the report pipeline (telemetry only — locked
fees stay 0 with `vault_fee = 0`), Manager/
Emergency/Rebalance-manager roles, `rescue()`/pause unwind-to-idle,
the factory — all OtterSec-audited (`defindex/audits/`). Our new-code
surface is the P1/P2 patch, the registry/platform contracts, and the
strategy contracts.

- **What a strategy is** (`apps/contracts/strategies/core/src/lib.rs`):
  six methods — `deposit`/`withdraw`/`balance` in underlying-asset terms,
  `harvest(from, data)` as the permissioned active-action hook ("claiming
  and compounding rewards, rebalancing a drifting position, rolling an
  expiring one" — their docs literally describe our rebalance). `data` is
  an opaque blob conventionally a min-out.
- **Mapping for CL** (the `cl_aquarius` reference strategy): single-asset
  **USDC** vault; the strategy internally owns the CL position — deposit
  swaps/pairs USDC→(token0,USDC)→`deposit_position`; `balance()` reports
  position value in USDC at the pool's own `sqrt_price_x96` (underlying
  value at live rate, never shares); `harvest` = keeper entry for
  claim-fees/compound/re-range; `withdraw` unwinds pro-rata + swaps back
  to USDC. (Vault asset decided USDC, 2026-10-25;
  `cl_vault_valuation.md` is the `balance()` design doc.)
- **What we keep from the prototype**: the whole strategy engine —
  keeper+`RebalancePlan` validation (allowlists, hops, expiry, tick
  drift, range width), `fair_in` slippage clamp, share_bps inventory
  band, atomic withdraw→swap→deposit — moves *inside* the strategy
  contract as init_args config + `harvest`-time checks.
- **Caveats to verify**: deposit/withdraw pairing swaps land per-user
  (mitigate by depositing at the position's current ratio, swapping only
  the deficit); `rescue()` must tolerate a full unwind (plus an
  `emergency_exit` that unwinds without swapping); strategy swaps route
  through **registry-listed venues** (Aquarius router for re-range — the
  vault's Soroswap `SwapExact*` path is neutralized, see
  `platform_design.md` §4.2); every price check references the Reflector
  oracle, not the pool's own spot; `from.require_auth()` needs a
  real-auth test for the vault→strategy→pool chain.

Why it matters specifically here: on the other chains the share-vault
absorbed the exact pain we measured on our own account (out-of-range =
zero yield, every manual rebalance leaking execution cost). Stellar has CL pools
(Aquarius, Sushi V3) but no share-vault product — today the only way to LP
is hand-managing positions. And beyond one vault: DeFindex proved the
pluggable-strategy vault on Stellar but shipped only lending
autocompounds — the CL strategy layer, and the platform that hosts many
of them, is open territory.

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

The synt_dex rebalancer prototype is **deployed and exercised on testnet**
(2026-10-02): vault `CBHV4KFGKKYJPS6CB65JCPERSWPTEO4AZJ5A7UYLAUJMTU2L2NELT4GA`
managing an XLM/AQUA CL position — opened, then rebalanced through the 1%
route pool to a 3055 bps token1 share. A first attempt through the 0.3%
pool correctly reverted: its price was ~38% off the CL reference, so the
required input exceeded the fair-price bound — the clamp caught a bad
route live.

- **Private contract-owned position**; `admin` (funds/policy) + `keeper`
  (submits `RebalancePlan`) — Gamma's `onlyOwner` + Revert's
  permissioned-executor hybrid.
- **Keeper plan fully re-validated on-chain**: pool/token allowlists, hop
  count, expiry, cooldown, `max_tick_delta` calm window, tick-spacing
  alignment, range-width bounds, `share_bps` inventory band.
- **Slippage bound from the managed pool's own price**:
  `effective_max_in = min(plan.max_in, fair_in × (1 + max_slippage_bps))`
  — the Sep-26 incident is what a missing bound costs. The prototype
  used the managed pool's own spot as the reference, which a keeper or
  anyone moving the pool in the same ledger can satisfy at a
  manipulated price; the public vault re-references `fair_in`, the loss
  cap and `balance()` to the Reflector oracle (`platform_design.md` §5.1).
- **Atomic rebalance in one tx**: withdraw → recompute exact `out_amount`
  → `swap_chained_strict_receive` → redeposit → verify band + tolerance +
  loss cap.
- **Trigger = range exit.** The 27-pool sweep found "rebalance at range
  exit" optimal on every pool measured; early triggers paid the swap leak
  2–3× more often for ≤ ~5% less divergence. Volatility-scaling belongs in
  the **width**, not the trigger.
- **Prototype was private; the product adds the share layer — on
  DeFindex's audited vault** — the custody/keeper/validation spine stays
  as built and testnet-proven (re-referenced to the oracle); the share
  minting, donation/inflation guards, fee accounting, roles and rescue
  come from their audited vault code (plus the P1/P2 correctness patch),
  deployed under our own factory instance. See "Don't reinvent".

## Venue adapters

| Venue | Status | Notes |
|---|---|---|
| Aquarius | interface live + documented | prototype deployed on testnet (see resources) |
| Sushi V3 | adapter needed | Uni-V3 semantics assumed; verify against deployed WASM |

## Resources already in this workspace

| Path | What it is |
|---|---|
| `rebalancing_vaults_research.md` | code-level survey: Gamma, Arrakis, Revert, Beefy, Charm, Steer, DeFindex — and what the prototype took/rejected |
| `cl_vault_landscape.md` | 7-type CL-vault taxonomy, trigger/swap-bound/position-shape/custody comparison tables, cautionary history |
| `cl_rebalance_strategies.md` | strategy-policy survey — the 5 families production vaults run, on-chain policy strictness ladder, `cl_band` comparison |
| `cl_strategy_templates.md` | design draft — `cl_core` shared crate + 5 family template WASMs on the platform; upstream code refs + rollout order |
| `keeper_models.md` | keeper economics survey — who calls rebalance on each platform, incentive models, Stellar options (permissionless bounty, Reflector trigger proofs) |
| `platform_design.md` | **the design draft** — platform spec: strategies-first decision, trust model, P1/P2 vault patch, registry/platform contracts, oracle-referenced swap policy, staging |
| `defindex_strategy_layer.md` | DeFindex deep dive — trait + vault mechanics, disadvantages & mitigations, fork-vs-plugin + platform fee model |
| `cl_vault_valuation.md` | `balance()` design — unwind-value recipe, oracle-manipulation defense, deposit pairing |
| `stellar/aquarius_dex/` | `aquarius_client.py`, `rebalance_solver.py`, `backtest_rebalance_policy.py`, pool PnL/cost analyzers, `pool_cache/`, live snapshots |
| `stellar/aquarius_dex/research/` | CLMM economics literature: LVR σ²/8, width/trigger sweeps, vol-regime experiments (README maps findings → design) |
| `security/Aquarius-security-review/` | Aquarius `soroban-amm` local working copy (upstream repo private — unmatched-to-deployed reference) |
| `synt_dex/rocky-rebalancer/` | **rocky-vault prototype** — contract (`contracts/rocky-vault/`), tests, testnet deployment notes (README), pinned reference sources under `reference/` (extracted from `synt_tech_website.zip`; gitignored, regenerable) |
| `rebalance_concentrated_positions_wrapper.py` | the manual rebalance flow the vault replaces |
| `../defindex/` | DeFindex local clone — audited vault + strategy trait (`apps/contracts/strategies/core/`) + Blend/hodl/soroswap example strategies; the share-vault infra we plug into |
| `reference/vault_docs/defindex/` | DeFindex docs corpus (76 pages): build-a-strategy guide, roles, fees, API |

## Open questions / TODO

- [ ] ~~Vault product shape~~ → decided: **self-operated platform on
      DeFindex's audited vault, strategies first** (see "Don't
      reinvent"). TODO: (a) vault patch — P1 loss-carry (test exists:
      `defindex/apps/contracts/vault/src/test/vault/loss_carry.rs`),
      P2 delivered-amount unwinds + post-unwind `min_amounts_out`;
      (b) `strategy_registry` (templates, venues, ceilings, bind) +
      `platform` wrapper + `null_router`; (c) `cl_aquarius` reference
      strategy — prototype policy engine re-referenced to Reflector
      (plan validation, fair_in clamp, share_bps band, loss budget),
      vault binding, RebalanceManager-gated `harvest`, pairing swaps on
      deposit/withdraw, clamped `balance()` in USDC,
      claim/compound/re-range, `emergency_exit`.
      Verify: real-auth tests for vault→strategy→pool call chains.
- [ ] Sushi adapter — how close to Uni-V3 semantics is the deployed pool?
      the prototype's own notes flag NFT positions, oracle hints and a
      different swap ABI as the deltas to resolve; skim `sushi-stellar-*`
      snapshots under `synt_dex/rocky-rebalancer/reference/` and diff
      against Aquarius's documented behavior.
- [ ] Keeper trigger: subscribe `trade`/`pool_state` events, fire only at
      range exit — vs polling loop.
- [ ] Multi-position (base + one-sided limit) to park leftover inventory
      without swapping — Gamma's reason for two positions.
- [ ] Rewards: `claim_rewards` exists in the prototype but was never
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
