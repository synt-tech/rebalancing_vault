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
  (`AI_sandbox/rebalance_concentrated_positions_wrapper.py`).

The measured cost of doing this badly, from our own account:

- **−719.91 sUSD in one day** (2026-09-26) from an unguarded rebalance swap.
- **+1,176 sUSD** fees earned vs **−918 sUSD** value destroyed over the whole
  post-Sep-26 manual run (net +258).
- Median execution leak ~3–3.5 sUSD/tx → a position must earn **more than
  that in fees between triggers** or rebalancing is negative-EV.

So the vault productizes order management: contract custody + policy
on-chain, strategy off-chain, atomic rebalances.

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
- **No public share tokens yet** — share inflation/donation and fee
  accounting are where the EVM corpses are buried.

## Venue adapters

| Venue | Status | Notes |
|---|---|---|
| Aquarius | interface live + documented | prototype existed as rocky-vault (see resources) |
| Sushi V3 | adapter needed | Uni-V3 semantics assumed; verify against deployed WASM |

## Resources already in this workspace

| Path | What it is |
|---|---|
| `AI_sandbox/rebalancing_vaults_research.md` | code-level survey: Gamma, Arrakis, Revert, Beefy, Charm, Steer, DeFindex — and what rocky-vault took/rejected |
| `AI_sandbox/cl_vault_landscape.md` | 7-type CL-vault taxonomy, trigger/swap-bound/position-shape/custody comparison tables, cautionary history |
| `AI_sandbox/stellar/aquarius_dex/` | `aquarius_client.py`, `rebalance_solver.py`, `backtest_rebalance_policy.py`, pool PnL/cost analyzers, `pool_cache/`, live snapshots |
| `AI_sandbox/stellar/aquarius_dex/research/` | CLMM economics literature: LVR σ²/8, width/trigger sweeps, vol-regime experiments (README maps findings → design) |
| `AI_sandbox/security/Aquarius-security-review/` | Aquarius `soroban-amm` local working copy (upstream repo private — unmatched-to-deployed reference) |
| `synt_tech_website.zip` | archive incl. `synt_dex/rocky-rebalancer/` — rocky-vault prototype + pinned reference sources (`reference/sources/`) |
| `AI_sandbox/rebalance_concentrated_positions_wrapper.py` | the manual rebalance flow the vault replaces |

## Open questions / TODO

- [ ] Vault product shape for the demo: private single position
      (rocky-vault shape) vs public share-token vault (needs
      donation/inflation guards + fee accounting).
- [ ] Sushi adapter — how close to Uni-V3 semantics is the deployed pool?
      (interface-verification task: skim `sushi-stellar-*` snapshots, diff
      against Aquarius's documented behavior.)
- [ ] Keeper trigger: subscribe `trade`/`pool_state` events, fire only at
      range exit — vs polling loop.
- [ ] Multi-position (base + one-sided limit) to park leftover inventory
      without swapping — Gamma's reason for two positions.
- [ ] Rewards: claim/re-stake AQUA rewards inside the vault, or leave to
      admin sweep? (note the checkpoint-stale-weight behavior above)
- [ ] Demo story: live testnet vault vs recorded live-pool replay?

## Links

- Hackathon: https://www.hackmeridian.com/
- Aquarius CL dev docs: https://docs.aqua.network/developers/concentrated-liquidity
  - interface ref: …/contract-interface-reference
- Sushi on Stellar: https://www.sushi.com/stellar/pool
- Stellar pool tracker: https://stellarindex.io/dexes/sushiswap_v3/
