# Testing

How the platform is verified today, and what remains to verify. Deployed
addresses and tx hashes are in [`../DEPLOYMENTS.md`](../DEPLOYMENTS.md).

## Run

```bash
cd contracts
cargo test -p cl_aquarius_strategy          # strategy suite (18 tests)
cargo test -p platform                      # deployment-path suite (2 tests)
cargo test -p defindex-vault                # patched vault suite (run inside ../defindex clone)
cargo build --target wasm32v1-none --release --workspace
```

The patched DeFindex vault lives in the sibling clone `../../defindex`
(branch `cl-vault-patch`); its suite is 108/108 including three
under-delivery tests added by the P1/P2 patch.

## Test matrix — `cl_aquarius` (18/18)

Mock pool / Reflector oracle / vault (`test_mocks.wasm`) plus the **real
patched `defindex_vault.optimized.wasm`** for the integration test.

| Test | Verifies |
|---|---|
| `deposit_parks_idle_and_balance_reports_it` | deposit pulls asset to strategy idle; `balance()` reports it; unbound callers get 0 |
| `withdraw_idle_delivers_requested` | idle-only withdrawal delivers the exact requested amount |
| `lp_fees_accrue_and_are_claimable` | LP-fee claim → `perf_fee_bps` skim → exact 50/50 platform/operator `fees_owed` split on both token legs; pull payout |
| `harvest_rejects_non_keeper` | `harvest` auth checks the vault's RebalanceManager/Manager role getters |
| `oracle_band_revert_blocks_harvest` | spot/oracle deviation beyond `oracle_band_bps` halts the rebalance |
| `emergency_exit_unwinds_without_swap` | position unwound to strategy custody, no swap needed |
| `rewards_converted_and_fee_skimmed_at_harvest` | reward claim → conversion swap at oracle-derived min-out → fee cut from swap output |
| `stale_oracle_blocks_withdraw` | oracle age > `max_oracle_age` reverts oracle-checked paths |
| `claim_fees_permissionless_pays_resolved_recipient` | anyone can trigger payout; destination is resolved on-chain, never caller-chosen |
| `rewards_below_min_swap_stay_unclaimed` | below `min_reward_swap` rewards stay unclaimed pool-side (pre-claim gate — no stranded custody dust); none land in strategy custody |
| `unclaimed_rewards_excluded_from_balance` | pending rewards do not move `balance()` — exiting holders can't be paid for forfeited rewards |
| `withdraw_does_not_claim_rewards` | withdrawal never calls `pool.claim` — rewards forfeit to remaining holders |
| `expired_plan_is_rejected` | `expiry < ledger time` → harvest error |
| `cooldown_blocks_second_harvest` | second harvest inside `cooldown_secs` fails; succeeds again past the window |
| `fee_legs_sum_exactly` | platform+operator legs sum exactly to `perf_fee_bps` of claimed income on both token legs; 50/50 split is exact |
| `vault_integration_deposit_invest_withdraw` | **real patched vault WASM**: deposit → keeper `Invest` → strategy pull → withdraw-all through the P2 actual-delivery path |
| `gen_swapplan_xdr`, `decode_plan_from_hex` | SwapPlan XDR encode/decode helpers for keeper tooling |

## Test matrix — `platform` (2/2)

Real `defindex_factory.wasm` + `defindex_vault.optimized.wasm` + registry,
null-router, strategy-template and mock-pool WASMs.

| Test | Verifies |
|---|---|
| `create_vault_deploys_binds_and_runs_deposit` | audit residual #1: `create_vault` → `deploy_strategy` → `create_defindex_vault` → `bind_vault` E2E — vault roles pinned (EmergencyManager=platform, Manager/FeeReceiver=caller, RebalanceManager=keeper), `init_args[2..3]` junk overridden by registry pinning, strategy bound with provenance recorded, live deposit through the created vault |
| `deploy_rejects_unlisted_pool` | audit residual A9: pool not in `Venues` → deploy reverts |

## Fuzzing

`contracts/cl_aquarius/fuzz/` is a standalone `cargo-fuzz` crate (own
`[workspace]`, own lockfile pinned to the workspace resolutions — see
`TOOLING.md`). Requires nightly. Commands from `contracts/cl_aquarius/`:

```bash
cargo +nightly fuzz run fuzz_plan_decode -- -max_total_time=300
cargo +nightly fuzz run fuzz_plan_structured -- -max_total_time=300
cargo +nightly fuzz run fuzz_deposit_withdraw -- -max_total_time=300
```

| Target | Input under test | Invariants |
|---|---|---|
| `fuzz_plan_decode` | arbitrary bytes → `try_harvest(keeper, Some(data))` | XDR decode + `validate_plan` + full harvest never panic in strategy code; any `Err` is legal |
| `fuzz_plan_structured` | `SorobanArbitrary` `SwapPlan` biased into the policy envelope (allowlisted venues, spacing-snapped ticks, boundary-straddling `out_min`/`expiry`) | same as above, but reaches post-decode internals; also `balance()` stays non-negative |
| `fuzz_deposit_withdraw` | `(i128 deposit, i128 withdraw)` | d ≤ 0 must be rejected; delivered ≤ min(w, deposited) ≥ 0; strategy balance in [0, deposit] |

Results so far (first campaigns, empty corpus):

| Target | Runs | Duration | Crashes | Coverage |
|---|---|---|---|---|
| `fuzz_plan_decode` | 229 | 90s | 0 | ~13.2k cov — stalls at the decode gate, as expected for raw bytes |
| `fuzz_plan_structured` | 369 | 120s | 0 | ~14.1k cov / 19.3k ft — reaches swap/redeploy internals |
| `fuzz_deposit_withdraw` | 3 379 | 90s | 0 | ~13.0k cov / 21.5k ft |

Harness findings fixed during setup (not contract bugs): the
deposit/withdraw fixture initially registered the pool with `sqrt=1`
(spot≈0) so the oracle-band check in `compute_balance` rejected every
deposit — corrected to the oracle-consistent √2.5·2⁹⁶. Two lazy-`Vec`-get
panics in harness code switched to `try_get`.

Generated corpus entries under `fuzz/corpus/` are kept as regression
seeds. `fuzz/artifacts/` and `fuzz/target/` are gitignored.

## Testnet smoke environment (verified flows)

Full stack deployed on Stellar testnet — see `DEPLOYMENTS.md` for all
addresses. Verified end-to-end against the patched vault WASM + mock
pool/oracle fixtures:

1. `platform.create_vault` — strategy deployed via registry (template
   hash + ceilings enforced), vault created by our factory instance
   (`vault_fee=0`, `upgradable=false`, `null_router`,
   EmergencyManager=platform), strategy↔vault bound.
2. `vault.deposit` 10e9 USDC → 9999999000 df shares (1000-unit
   dead-share lock on first deposit).
3. `vault.rebalance` `Invest` → strategy pulls 8e9.
4. `strategy.harvest` — XDR `SwapPlan` decoded on-chain, policy validated,
   position deployed at ticks [-100,100] via `deposit_position`.
5. Reward conversion — seeded 2e9 AQUA reward → claimed → swapped
   AQUA→XLM through the allowlisted reward pool at oracle-derived min-out
   → `FeeAccrued` event: 15% skim split 72750000/72750000
   platform/operator.
6. `vault.withdraw` all shares → P2 unwind through the open position →
   user received 9999999000.

## Coverage gaps (next tests to write)

- ~~**Fork test**~~ — DONE. `test_fork::fork_real_pool_full_flow` passes
  end-to-end on a real mainnet snapshot (ledger ~65.14M): live
  Reflector prices via per-asset routes, whale-funded deposit,
  unwind-only harvest, redeploy into a live-tick-derived single-sided
  USDC band (`[-18380,-17880]` at tick −17872), balance, and a partial
  withdraw through the open real position — with the strategy running
  as its **compiled WASM** and per-op `InvocationResources` printed
  (redeploy 156M instr, withdraw 275M instr vs the 400M tx cap).
  See `testing/fork/README.md`.
- **Position unwind path in `withdraw` (mock coverage)** — the fork test
  exercises it on the real pool; a mock that returns position value
  would add cheap per-commit coverage.
- **Under-delivery through the vault** — `leaky_strategy` covers it in
  the defindex clone; an equivalent cl_aquarius-side case would exercise
  the P2 `min_amounts_out`-on-delivered check end-to-end.
- **Plan rejection cases**: non-allowlisted venue, out_min violating the
  oracle-derived bound, tick range outside
  `range_min_bps`/`range_max_bps`, `|new_tick − ref_tick| > tick_cap_bps`,
  > `MAX_SWAP_STEPS` hops. (Expired plans and the cooldown are covered.)
- **Loss budget** — consecutive-loss accumulation within the window and
  window reset.
- **Atomic revert** — mid-harvest failure (e.g. swap step reverting)
  leaves the whole call reverted, position restored.
- **`set_policy`** — registry ceiling enforcement on operator policy
  changes.
- **Two-asset vault config** — in-kind rescue path (`rescue` delivering
  both pool tokens).

## Known caveats / deliberately out of scope

- `harvest` exceeds the default test budget (auth entries + full path);
  tests run with `reset_unlimited`. Real-pool profiling (fork test,
  strategy-as-WASM): deposit 5.3M / unwind 15.5M / redeploy 156.5M /
  withdraw 274.8M modeled instructions vs the 400M mainnet tx cap —
  all fit; withdraw is the heavy path (~69%). Keep harvest+withdraw out
  of single transactions in keeper design.
- `balance()` assumes equal token decimals (XLM/USDC are both 7dp).
- Withdrawal-before-harvest: uncollected LP fees are fee-netted in
  `balance()`; unclaimed rewards are excluded entirely (exiting holder
  forfeits them) — documented design choice.
- Oracle prices on the mock must be re-seeded (`set_price`) before any
  oracle-checked call if `max_oracle_age` has elapsed.
