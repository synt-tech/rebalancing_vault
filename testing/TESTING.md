# Testing

How the platform is verified today, and what remains to verify. Deployed
addresses and tx hashes are in [`../DEPLOYMENTS.md`](../DEPLOYMENTS.md).

## Run

```bash
cd contracts
cargo test -p cl_aquarius_strategy          # strategy suite (12 tests)
cargo test -p defindex-vault                # patched vault suite (run inside ../defindex clone)
cargo build --target wasm32v1-none --release --workspace
```

The patched DeFindex vault lives in the sibling clone `../../defindex`
(branch `cl-vault-patch`); its suite is 108/108 including three
under-delivery tests added by the P1/P2 patch.

## Test matrix — `cl_aquarius` (12/12)

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
| `vault_integration_deposit_invest_withdraw` | **real patched vault WASM**: deposit → keeper `Invest` → strategy pull → withdraw-all through the P2 actual-delivery path |
| `gen_swapplan_xdr`, `decode_plan_from_hex` | SwapPlan XDR encode/decode helpers for keeper tooling |

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

- **Position unwind path in `withdraw`** — currently only idle-funded
  withdrawal is unit-tested; pro-rata `withdraw_position` + foreign-leg
  conversion needs a mock that returns position value.
- **Under-delivery through the vault** — `leaky_strategy` covers it in
  the defindex clone; an equivalent cl_aquarius-side case would exercise
  the P2 `min_amounts_out`-on-delivered check end-to-end.
- **Plan rejection cases**: non-allowlisted venue, `expiry` past,
  out_min violating oracle-derived bound, tick range outside
  `range_min_bps`/`range_max_bps`, `|new_tick − ref_tick| > tick_cap_bps`.
- **Cooldown** second-harvest rejection + success after `cooldown_secs`.
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
  tests run with `reset_unlimited`. **Real-pool resource profiling is a
  pending calibration task**, not a passing assumption.
- `balance()` assumes equal token decimals (XLM/USDC are both 7dp).
- Withdrawal-before-harvest: uncollected LP fees are fee-netted in
  `balance()`; unclaimed rewards are excluded entirely (exiting holder
  forfeits them) — documented design choice.
- Oracle prices on the mock must be re-seeded (`set_price`) before any
  oracle-checked call if `max_oracle_age` has elapsed.
