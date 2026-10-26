# Audit notes

Threat model, trust assumptions, and review checklist for the platform.
Read together with `../DEPLOYMENTS.md` (deployed addresses) and
`../platform_design.md` in the workspace root (design rationale).

## Components under review

| Contract | Role | Diff surface |
|---|---|---|
| `defindex/apps/contracts/vault` (branch `cl-vault-patch`) | Custody + shares | **Patched**: `unwind_from_strategy` measures actual delivery and always delivers to vault; `withdraw` enforces `min_amounts_out` on delivered and pays withdrawer the delivered amount; `invest_in_strategy` accumulates `gains_or_losses` (P1); `rescue`/`rebalance`/report updated |
| `defindex/apps/contracts/factory` | Vault deployer | Stock; our own instance (protocol fee 0, receiver=ops) |
| `cl_aquarius` | CL strategy | New |
| `strategy_registry` | Template provenance + deploy + ceilings | New |
| `platform` | `create_vault` orchestration + emergency ops | New |
| `null_router` | Kill the vault's Soroswap manager-swap path | New (258 B) |

## Trust assumptions

1. **Aquarius pool WASM is admin-upgradeable with a 3-day delay** — a
   compromised/malicious Aquarius admin can change pool behavior. Pools
   also have per-pool kill switches (`deposit_killed`/`swap_killed`/
   `claim_killed`) and an emergency mode the strategy checks before
   position ops.
2. **Reflector oracle** is trusted for prices; liveness is bounded by
   `max_oracle_age` and correctness bounded by `oracle_band_bps` vs pool
   spot. A stale-but-fresh-enough manipulated feed inside the band is a
   residual risk — bounded by `max_slippage_bps` on every swap min-out.
3. **Keeper (RebalanceManager)** proposes `SwapPlan`s — "keeper proposes,
   contract disposes": every step's venue must be allowlisted, every
   `out_min` is checked against the oracle price × `(1−max_slippage_bps)`,
   `ref_tick` is checked against `tick_cap_bps`, ranges are bounded by
   `range_min_bps`/`range_max_bps` + tick spacing.
4. **Operator (vault Manager / FeeReceiver)** cannot move funds except
   through the strategy trait surface; `vault_fee` pinned 0 and
   `upgradable=false` remove the two direct value-extraction paths.
   Phase-2 `manager_proxy` narrows the role further.
5. **Platform admin** holds: registry admin (templates, venues, ceilings),
   factory admin, EmergencyManager on all platform vaults. It can pause
   strategies and `rescue` funds to vault idle — but cannot redirect
   withdrawals; depositors exit at the normal share price.

## Authorization model (verified on testnet)

- Soroban invoker auth covers only the **direct** callee's
  `require_auth`. Any contract pulling tokens *through* another contract
  must pre-authorize the subtree: the DeFindex vault does this for
  strategy deposits (`invest_in_strategy`), and `cl_aquarius` does it via
  `authorize_pull` before `deposit_position` and every `swap` call.
  `mock_all_auths` hides this — **only real-network calls surface it**.
  Any new token-pulling call path needs the same treatment.
- `cl_aquarius.bind` is one-shot and requires `caller == registry`;
  the registry requires its `platform` caller (invoker auth) for
  `bind_vault`.
- `claim_fees` is permissionless *by design*: anyone may trigger it,
  but destinations are resolved on-chain (platform → registry treasury,
  operator → `vault.get_fee_receiver()`), so callers cannot redirect.
- `harvest` requires `from` to be the vault's RebalanceManager or Manager
  (queried live from vault role getters, not cached).

## Accounting invariants worth asserting in review/tests

- `fees_owed` (platform + operator) is **never spendable capital**:
  `spendable() = balance − owed_platform − owed_operator` on every idle
  read; `balance()` excludes it; withdraw/harvest can never pay it out.
- Fee is cut **once**, on realized income only: LP fees at claim time
  (`perf_fee_bps`), reward conversion output (same bps, from swap output).
  `platform + operator` legs always sum to the fee exactly.
- `balance()` = spendable idle + oracle-valued position + uncollected LP
  fees net of `perf_fee_bps`; unclaimed rewards are excluded by policy.
- Vault `withdraw` pays the withdrawer the **delivered** amount;
  under-delivery is borne by the withdrawer via `min_amounts_out`, not
  socialized across holders (P2).

## Resolved since first review

- ~~`min_reward_swap` gate ran post-claim, stranding below-threshold
  rewards as unvalued custody dust that could never retry.~~ Fixed: the
  gate now reads `get_user_reward` and skips claiming entirely below the
  threshold — rewards accrue pool-side. (`rewards_below_min_swap_unclaimed`)
- ~~Deployment path untested (residual #1).~~ Covered:
  `platform/src/test.rs` runs `create_vault` E2E against real
  factory+vault WASMs with junk `init_args[2..3]` proving pinning.
- ~~§9 reward/edge cases untested.~~ Covered: unclaimed-excluded-from-
  `balance()`, withdraw-never-claims, expired plan, cooldown + recovery,
  fee-leg conservation.

## Static analysis (Scout / cargo-scout-audit 0.3.17)

- First run: **37 critical** (unchecked arithmetic) + 41 medium on
  `cl_aquarius`. Fixed: all arithmetic on externally-derived magnitudes
  (oracle prices, pool estimates, keeper amounts, claimed/pending
  rewards, balances, `BPS − policy_*_bps` keep factors) now uses checked
  ops returning `UnderflowOverflow`/`DivisionByZero` — an adversarial
  pool/oracle value errors instead of trapping. `spendable` and
  `add_owed` saturate.
- Second run: **0 critical** on every production crate; `test_mocks`
  keeps its own findings (test fixture).
- Remaining mediums triaged — none exploitable:
  - 46 `unwrap` flags: `tokens.get(0)`-style reads on malformed external
    responses panic → revert, semantically equal to a typed error.
  - 5 "unbounded ops": `plan.steps` is capped at `MAX_SWAP_STEPS = 5`;
    registry loops iterate admin-curated lists or the caller's own args
    (self-DoS only).
  - "Vec/Map params unvalidated": `deploy_strategy`/`create_vault`
    *do* validate `init_args` shape, venues, and policy.
  - `extend_ttl(100_000, 100_000)` identical args — deliberate.

## Tooling status

`cargo-scout-audit` 0.3.17 installed and run locally — see
`TOOLING.md` for the full ecosystem inventory (fuzz, fork testing,
formal verification).

## Known weaknesses / follow-ups

- [ ] Cooldown breach returns `NotAuthorized`; should be a distinct error code.
- [ ] `balance()`/`harvest` assume equal token decimals — fine for
      XLM/USDC (7dp); must generalize before other pairs.
- [ ] Loss budget measures harvest-to-harvest balance delta, which
      includes market price drift — not pure execution loss. `ref_tick`
      validation narrows this; revisit if it false-triggers in volatile
      markets.
- [ ] `idle_funds` in the vault accrues any under-delivery remainder —
      flagged in the P2 patch; harmless while strategies can't
      under-deliver at deposit-time.
- [ ] Reward conversion assumes a single-hop `reward_pool` pairing the
      reward token with a managed token; multi-hop rewards (or
      reward_token == managed token edge case) not handled.
- [ ] `emergency_exit` keeps both pool tokens in strategy custody —
      claimable later, but no `claim_fees`-style rescue entry point for
      them yet; withdrawal flow assumes a working conversion path unless
      the vault lists both tokens (in-kind rescue option).
- [ ] No reentrancy guard needed (Soroban model), but cross-contract call
      ordering in `harvest` (claim → unwind → swap → redeploy → post
      oracle check) should be re-reviewed for sandwich surface inside the
      band.
- [ ] Registry `deploy_strategy` parses `init_args[4]` as `Policy` —
      brittle to constructor-arg reordering; add a shape/version check.
- [ ] `platform.pause_strategy`/`unpause_strategy`/`rescue_vault` are
      admin-only but their auth flow on-chain is exercised only via
      EmergencyManager role — add testnet verification.
- [ ] A missing `reward_token` oracle price aborts `harvest` entirely
      (`claim_and_convert_rewards` propagates) even though the managed-pool
      band check already passed — reward conversion is opportunistic and
      could be made failure-isolated if that proves too strict.
- [ ] Fee receiver without a pool-token trustline and AQUA conversion
      under-delivering below oracle min-out — not reproducible with the
      current mocks (SAC trustlines always exist in tests; the mock pool
      always pays `out_min`). Needs a mock knob or fork test.

## Not yet audited

- Soroban **resource-budget profile** of the full harvest path on a real
  pool (claim → withdraw → swap → oracle reads → fee conversion →
  redeploy) — pending; the mock already exceeds the default test budget.
- Real Aquarius testnet pool integration (current smoke env uses mocks).
- Parameter calibration (`perf_fee_bps`, `platform_split_bps`,
  `oracle_band_bps`, `max_oracle_age`, `max_loss_bps_per_window`,
  `min_reward_swap`, `max_reward_age`) — pending fork/testnet runs.
