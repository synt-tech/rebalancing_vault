# Contract audit — CL rebalancing platform

Status: **internal engineering review**, not a third-party security audit or
formal verification. It covers the contracts and tests in this workspace and
records the findings that were fixed in commit `1e15c13`
(`contracts: harden harvest validation, deployment wiring, fee events`).

- Audit date: 2026-10-26
- Design baseline: `platform_design.md` (repo root), revised 2026-10-25
- DeFindex baseline: tag `1.0.0`, commit `f2d2785` (audited release)
- Fixed-state commit: `1e15c13`
- Scope: `hackmeridian_rebalancing_vault/contracts/` plus the P1/P2 vault
  patch already present in `defindex/apps/contracts/vault/`

## 1. Scope

### Reviewed

| Area | Path |
|---|---|
| Strategy | `contracts/cl_aquarius/src/{lib.rs,cl_core.rs,clients.rs,storage.rs}` |
| Strategy tests | `contracts/cl_aquarius/src/test.rs` |
| Platform wrapper | `contracts/platform/src/lib.rs` |
| Registry | `contracts/strategy_registry/src/lib.rs` |
| Null router | `contracts/null_router/src/lib.rs` |
| Test mocks | `contracts/mocks/src/lib.rs` |
| Vault patch (P1/P2) | `defindex/apps/contracts/vault/src/{strategies.rs,report.rs}` |
| Vault patch tests | `defindex/apps/contracts/vault/src/test/vault/{loss_carry.rs,under_delivery.rs}` |
| Design contract | `platform_design.md` |

### Out of scope

- Off-chain solver, keeper service, frontend, indexer, and alerting.
- Economic calibration of `perf_fee_bps`, `oracle_band_bps`,
  `max_oracle_age`, `max_loss_bps_per_window`, `min_reward_swap`, and
  `max_reward_age`. These remain testnet calibration items.
- Full execution of the upstream DeFindex vault suite. The P1/P2 patch and
  its tests were inspected, not re-run in this review.
- Mainnet deployment state, auth traces, and resource-budget profiling with
  real accounts.

## 2. Architecture reviewed

The implementation follows the design's split:

- **`cl_aquarius`** is a single-vault, single-position CL strategy. It parks
  deposits as strategy-held idle, deploys into the band at `harvest`, claims
  and skims LP fees/rewards, and supports `emergency_exit` without a swap.
- **`strategy_registry`** owns template provenance, venue allowlists, policy
  ceilings, deployment, and one-shot vault binding.
- **`platform`** creates vaults through the stock factory with
  `vault_fee = 0`, `null_router`, `upgradable = false`, and the platform as
  `EmergencyManager`, then binds the strategy.
- **`null_router`** makes the vault's manager-swap path fail closed.
- **`defindex` vault patch** fixes loss-carry accumulation (P1) and measures
  delivered amounts on unwinds and fee distribution (P2).

## 3. Findings and disposition

Severity: **Critical** = funds or authorization failure; **High** = policy
bypass, deployment failure, or accounting corruption; **Medium** = operational
or indexer impact; **Low** = documentation/interface hygiene.

### 3.1 Resolved in `1e15c13`

| ID | Severity | Finding | Disposition |
|---|---|---|---|
| A1 | Critical | `harvest` executed keeper-supplied `in_amount`/`out_min` without re-deriving the bound from the oracle. A keeper could fill a plan at a price inside the oracle band but worse than the policy's slippage cap. | `cl_core::validate_plan` recomputes `fair_out` from the oracle for every leg and requires `out_min >= fair_out * (1 - max_slippage_bps)`. |
| A2 | Critical | `set_policy` accepted any policy shape; registry ceilings and venue allowlists were only enforced at deployment. | `cl_aquarius::set_policy` calls `RegistryClient::validate_policy`; the registry enforces ceilings, `min_platform_split_bps`, and venue membership. |
| A3 | Critical | `strategy_registry::deploy_strategy` passed the operator's 7-element `init_args` directly to `deploy_v2`, while the strategy constructor is `__constructor(asset, init_args)`. The constructor would receive the wrong shape. | The registry now takes `asset` and builds `deploy_args = [asset, init_args]`. `platform.create_vault` passes `asset` through. |
| A4 | Critical | Operators could set `init_args[2]` (registry) and `init_args[3]` (treasury), redirecting strategy binding and the platform fee leg. | The registry pins `init_args[2]` to its own address and `init_args[3]` to its stored treasury before policy validation and deployment. |
| A5 | Critical | `platform.create_vault` invoked `registry.bind_vault`, which calls `platform.require_auth()`, without the platform authorizing its own contract invocation. This would fail in production. | `platform.create_vault` now calls `authorize_as_current_contract` for the `bind_vault` invocation. |
| A6 | Critical | No `FeeAccrued` / `FeePaid` events. The indexer and API could not attribute income or payouts. | `cl_core::skim` emits `FeeAccrued` with source (`lp_fees` or `rewards`); `claim_fees` emits `FeePaid`. |
| A7 | Critical | `claim_fees` required the caller to equal the recipient and used a stored treasury instead of the registry treasury. This conflicted with the design's permissionless pull and "platform → registry treasury" rule. | `claim_fees` is permissionless; it resolves the platform recipient from `registry.treasury()` and the operator recipient from the vault's fee receiver at payout time. |
| A8 | Critical | Registry `check_policy` had no floor on `platform_split_bps`; an operator could set it to zero and take the whole performance fee. | `Ceilings` gained `min_platform_split_bps`; `check_policy` enforces it. |
| A9 | High | `deploy_strategy` validated only `policy.swap_allowed`, not the pool/oracle/reward-pool constructor arguments. A reviewed template could be pointed at a hostile pool or oracle. | The registry validates `init_args[0]` (pool), `[1]` (oracle), and `[6]` (reward pool) against `Venues`. `asset` is validated by the strategy constructor against the pool tokens. |
| A10 | High | `tick_cap_bps` was defined in `Policy` but unused; `new_range` could be deployed arbitrarily far from the keeper's reference. | `validate_plan` checks `abs(center - ref_tick) <= tick_cap_bps`. |
| A11 | High | `SwapPlan` lacked `ref_tick`, so the design's `max_tick_delta` validation had no reference. | Added `ref_tick: i32` to `SwapPlan`. This is an XDR-breaking change; the off-chain keeper and plan builders must include it. |
| A12 | Medium | `withdraw` and `harvest` could call into a pool that had entered emergency mode. | Both paths now revert with `ExternalError` when `pool.get_emergency_mode()` is true. `emergency_exit` remains available for rescue. |
| A13 | Medium | Missing strategy events: `deposit`, `withdraw`, `Rebalance`. | Added `emit_deposit`, `emit_withdraw`, and a `Rebalance` event carrying the keeper, step count, and new range. |
| A14 | Medium | `deploy_strategy` did not require the deployer's authorization; anyone could deploy from a registered template. | Added `deployer.require_auth()`. |
| A15 | Medium | Registry and platform events were missing. | Added `TemplateRegistered`, `VenueRegistered`, `StrategyDeployed`, `StrategyBound`, and `VaultListed`. |
| A16 | Medium | The real-vault integration test asserted the full deposit amount as the user's share balance and ran into the default Soroban test budget on the reward path. The vault's `MINIMUM_LIQUIDITY = 1000` dead shares make the first mint smaller. | Test assertions now respect dead shares; heavy tests reset the cost-estimate budget. |
| A17 | Low | The XDR fixture in `decode_plan_from_hex` predated `ref_tick`. | Fixture regenerated for the four-field `SwapPlan`; decode test now asserts success. |

### 3.2 Accepted / informational

| ID | Severity | Observation | Note |
|---|---|---|---|
| A18 | Info | `claim_fees` treats any `role != 0` as operator. | Documented interface is `0 = Platform`, `1 = Operator`. A `role > 1` currently pays the operator leg. Fine for v0; reject unknown roles if the interface is exposed directly to users. |
| A19 | Info | `withdraw` sizes the unwind by `pos.liquidity * amount / total`. | For a single CL range the token amounts are linear in liquidity, so this is exact for pro-rata withdrawal. Range-crossing execution cost remains borne by the exiting user and bounded by `min_amounts_out` + the oracle guard. Revisit with multi-range positions. |
| A20 | Info | `emit_harvest` still passes `0` for the harvest amount. | `FeeAccrued` now carries the income and fee split, so the indexer can attribute fees. Populating the DeFindex harvest amount is a follow-up. |
| A21 | Info | The real-vault integration test uses `mock_all_auths_allowing_non_root_auth()`. | Better than `mock_all_auths`, but still not the design's real-auth chain (`vault → strategy → pool → token`). Staging item. |

## 4. Verification

Commands run in `hackmeridian_rebalancing_vault/contracts/`:

```bash
cargo check --workspace
cargo build --release --target wasm32v1-none --workspace
cargo test -p cl_aquarius_strategy
```

Results:

- `cargo check --workspace` — clean.
- `cargo build --release --target wasm32v1-none --workspace` — clean.
- `cargo test -p cl_aquarius_strategy` — **12/12 pass**:
  `deposit_parks_idle_and_balance_reports_it`,
  `withdraw_idle_delivers_requested`,
  `lp_fees_accrue_and_are_claimable`,
  `rewards_converted_and_fee_skimmed_at_harvest`,
  `stale_oracle_blocks_withdraw`,
  `claim_fees_permissionless_pays_resolved_recipient`,
  `harvest_rejects_non_keeper`,
  `oracle_band_revert_blocks_harvest`,
  `emergency_exit_unwinds_without_swap`,
  `vault_integration_deposit_invest_withdraw`,
  `gen_swapplan_xdr`,
  `decode_plan_from_hex`.

The P1/P2 patch was inspected in the `defindex` clone:

- P1: `strategies.rs:194` accumulates `gains_or_losses` instead of assigning.
- P2: `strategies.rs:140` `unwind_from_strategy` returns
  `(delivered, remaining_balance)`; `report.rs:189`
  `distribute_strategy_fees` pays `min(locked_fee, delivered)`.
- Tests present: `test/vault/loss_carry.rs`, `test/vault/under_delivery.rs`.
  These were not re-executed in this review.

## 5. Residual risks and open items

Priority order:

1. **Runtime-test the registry/platform deployment path.** The constructor
   shape (`[asset, init_args]`), `init_args` pinning, deployer auth, platform
   self-auth for `bind_vault`, and factory role wiring compile but are not
   exercised by a test. This is the highest-value next test.
2. **Real-auth E2E.** Replace `mock_all_auths_allowing_non_root_auth()` in
   `vault_integration_deposit_invest_withdraw` with the actual
   `vault → strategy → pool → token` auth chain.
3. **§9 edge cases still missing.** Fee receiver without a pool-token
   trustline; AQUA conversion below the oracle min-out; fee-leg
   conservation/rounding; exit-before-harvest vs exit-after; forfeited
   unclaimed rewards. The fee accounting code says these behave as designed,
   but the tests are absent.
4. **`SwapPlan.ref_tick` migration.** Off-chain solver/keeper plan builders
   must add the field; old XDR plans will fail to decode.
5. **Admin setup for the new registry fields.** `set_ceilings` must include
   `min_platform_split_bps`; the oracle must be registered in `Venues` for
   `deploy_strategy` to accept a strategy. Document this in the deployment
   runbook.
6. **`emit_harvest` amount.** Populate the DeFindex harvest event from the
   realized income if downstream indexers rely on it.
7. **Role validation in `claim_fees`.** Reject `role > 1` if the entry point
   is exposed as a public convenience API.
8. **Calibrate policy parameters on testnet.** `perf_fee_bps`,
   `oracle_band_bps`, `max_oracle_age`, `max_loss_bps_per_window`,
   `min_reward_swap`, `max_reward_age`.
9. **P1 disclosure.** The design doc calls for responsible disclosure to
   PaltaLabs before publishing the loss-carry finding.
10. **Formal audit.** This review is not a substitute for a third-party
    audit, formal verification, or a mainnet dry run. The vault patch is a
    GPL-3.0 derivative of DeFindex; new contracts and `cl_core` remain outside
    the upstream audit boundary.

## 6. Key code locations

| Concern | Location |
|---|---|
| Strategy constructor / vault binding | `contracts/cl_aquarius/src/lib.rs:80`, `:344` |
| Deposit / withdraw | `contracts/cl_aquarius/src/lib.rs:121`, `:139` |
| Harvest + plan validation | `contracts/cl_aquarius/src/lib.rs:227`, `cl_core.rs:173` |
| Fee accrual / payout | `contracts/cl_aquarius/src/cl_core.rs:137`, `lib.rs:361` |
| Registry deploy / bind | `contracts/strategy_registry/src/lib.rs:180`, `:256` |
| Platform vault creation | `contracts/platform/src/lib.rs:93` |
| P1 accumulation | `defindex/apps/contracts/vault/src/strategies.rs:194` |
| P2 delivered-amount accounting | `defindex/apps/contracts/vault/src/strategies.rs:140`, `report.rs:189` |

## 7. Change record

`1e15c13` — `contracts: harden harvest validation, deployment wiring, fee events`

Files: `cl_aquarius/src/{cl_core.rs,clients.rs,lib.rs,storage.rs,test.rs}`,
`mocks/src/lib.rs`, `platform/src/lib.rs`, `strategy_registry/src/lib.rs`.

The commit message records the pre-fix failure modes and the verification
commands above.
