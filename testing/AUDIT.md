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
   Verified live on mainnet: no single public feed covers every asset —
   XLM/USDC exist only on the external CEX+DEX USD feed
   (`CAFJZQWSED6YAWZU3GWRTOCNPPCGBN32L7QV43XX5LZLFTK6JLN34DLN`, identifier
   `Other(symbol)`, e.g. XLM=$0.1673 USDC=$1.0011), AQUA only on the
   Stellar-DEX feed (`CALI2BYU…`, identifier `Stellar(sac)`,
   base=USDC). The strategy therefore routes `oracle_price` per-asset via
   `OracleOf`/`OracleAssetOf` set at init (`init_args[7] =
   Vec<OracleRoute>`), defaulting to the init oracle + `Stellar(asset)`.
   `PriceData.timestamp` is seconds-scaled — `max_oracle_age` works.
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

- ~~**Inverted oracle ratio convention** (`ref_price_1e14`,
  `check_oracle_band`, `withdraw` conversion): `ref_px` was documented as
  "units of token1 per token0" but computed as `p1/p0` — the *inverse*.
  `check_oracle_band` compared pool spot (units t1/t0) against that
  inverted ratio, so on a real pool (sorted tokens, e.g. t0=USDC $1,
  t1=XLM $0.40 → fair units 2.5 XLM/USDC) it computed `0.4` vs spot `2.5`
  and would revert **every** oracle-gated path (deposit, withdraw,
  harvest). `compute_balance`'s conversions were also inverted under that
  convention. The mock fixture masked the bug: it set
  `sqrt = √2.5` — self-consistent with the inverted ratio, not with real
  economics — and no test asserted a value that distinguishes the two
  conventions (idle-only USDC paths are conversion-vacuous). Fixed:
  `ref_px` now genuinely means units-of-t1-per-unit-t0 (`p0/p1`),
  matching its doc comment, the spot direction, and `compute_balance`;
  `withdraw`'s two conversion arms were swapped to match. Fixtures
  updated to `√0.4·2⁹⁶`. **Exactly the bug class fork testing exists to
  catch** — a unit-test fixture that is internally consistent cannot
  detect it; only real-pool state does.
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
- [x] ~~`emergency_exit` keeps both pool tokens in strategy custody~~ —
      `rescue(caller)` now delivers both legs in-kind to the vault
      (EM/Manager auth). Platform-side passthrough still unwired —
      call `rescue` directly for now.
- [ ] No reentrancy guard needed (Soroban model), but cross-contract call
      ordering in `harvest` (claim → unwind → swap → redeploy → post
      oracle check) should be re-reviewed for sandwich surface inside the
      band.
- [ ] Registry `deploy_strategy` parses `init_args[4]` as `Policy` —
      brittle to constructor-arg reordering; add a shape/version check.
- [x] ~~`platform.pause_strategy`/`unpause_strategy`/`rescue_vault`
      testnet verification~~ — done; surfaced and fixed the missing
      caller-arg/invoker-auth bug (platform v2 `CDKAKRSZ…`, txs
      `7eb70987…`/`586ab9a9…`).
- [x] ~~A missing `reward_token` oracle price aborts `harvest`~~ —
      now failure-isolated: stale/missing reward feed skips the claim
      (rewards keep accruing pool-side).
- [ ] Fee receiver without a pool-token trustline and AQUA conversion
      under-delivering below oracle min-out — not reproducible with the
      current mocks (SAC trustlines always exist in tests; the mock pool
      always pays `out_min`). Needs a mock knob or fork test.

## Audited since — real-mainnet fork results (2026-10-26)

- **Resource-budget profile** — DONE on real WASMs. InvocationResources
  vs live `tx_max_instructions`=400M / `tx_memory_limit`=40MiB:
  deposit 5.3M/4.9MB (1.3%), unwind 15.5M/13.4MB (3.9%),
  redeploy 156.5M/23.6MB (39%), withdraw 274.8M/25.9MB (69%). All fit;
  document that compound keeper ops must stay one-call-per-tx.
- **Parameter sanity** — the default policy envelope (oracle band, loss
  budget, cooldowns, reward thresholds) accepted a live-state deposit,
  unwind, single-sided redeploy, and withdraw without tripping — a first
  calibration pass on real data. Deeper tuning (band width, cadence)
  remains a keeper-economics question, not a correctness one.
- **Dependency-closure lesson** — the real pool internally `try_call`s
  boost-plane contracts (`RewardBoostToken.balance`,
  `RewardBoostFeed.total_supply`, `Plane.update`); missing instances
  escalate `Storage MissingValue`→`Abort`. Enumerate externals from the
  target's instance storage before snapshotting.

## Found on testnet — platform emergency-ops bug (fixed, v2 deployed)

- **Bug** — `platform.pause_strategy`/`unpause_strategy`/`rescue_vault`
  called the vault with `[strategy]` (1 arg) but the vault ABI is
  `(strategy_address, caller)` — live testnet failure:
  `WasmVm UnexpectedSize` / `MismatchingParameterLen`. Even at the right
  arity, `caller.require_auth()` inside `require_any_role` needs the
  platform to declare the sub-invocation via
  `authorize_as_current_contract`. Slipped through because no test ever
  drove the platform→vault emergency path (unit tests exercised
  `create_vault` only).
- **Fix** — `call_emergency` helper: `[strategy, platform]` args +
  `InvokerContractAuthEntry` for the vault call (same pattern as the
  `bind_vault` callback). Covered by the extended `create_vault` test
  (pause → unpause → rescue against the real vault WASM).
- **Deployed** — platform v2 `CDKAKRSZ…` (wasm `fcb1611c…`);
  `registry.set_platform` re-pointed (`8aec32db…`); v4 vault
  `set_emergency_manager` (`e5040e5d…`); verified live pause
  (`7eb70987…`) + unpause (`586ab9a9…`) on testnet.
- **Auth-model note** — the host also auto-authorizes `require_auth` on
  the *direct invoker* contract's address (soroban-env-host
  `maybe_check_invoker_contract_auth`), so the declared entry is
  belt-and-suspenders for non-direct call paths; the arg fix is the
  load-bearing change.
- **Lesson** — every wrapper-level proxy method needs at least one
  end-to-end exercise against the real callee WASM; mock-auth unit
  tests cannot catch arity or invoker-auth gaps on their own.

## Found in the coverage push (2026-10-26, all fixed + tested)

- **Stale reward feed bricked harvest** — `claim_and_convert_rewards`
  propagated `oracle_price(reward_token)` failure, so a dead/stale AQUA
  feed reverted every harvest while the managed-pair feed was healthy.
  Now skips the claim like the dust gate — rewards keep accruing
  pool-side. Test: `stale_reward_oracle_skips_claim`.
- **`LastBalance` never seeded on deposit** — the loss-budget baseline
  only existed after the first harvest (`unwrap_or(new_bal)` recorded
  zero loss for it). `deposit` now stores the computed balance so the
  first harvest's loss is measured against post-deposit value.
- **Withdrawals mis-charged the loss budget** — `LastBalance` was not
  refreshed on `withdraw`, so a vault-side withdrawal between harvests
  would be booked as keeper loss on the next harvest. Post-withdraw
  balance is now stored (skipped on oracle failure — `balance()`
  fallback covers it).
- **No in-kind rescue path existed** — `withdraw` hard-requires the
  foreign-leg swap, so on a halted venue even the asset leg was locked
  (`emergency_exit` only moved funds to strategy custody). Added
  `strategy.rescue(caller) -> Vec<i128>`: EmergencyManager/Manager auth,
  transfers all *spendable* balances of both pool tokens to the vault
  raw (vault does not share-account the foreign leg — custody recovery
  is the point). Tests: `rescue_delivers_both_legs_in_kind`,
  `rescue_rejects_unauthorized`. Deployed strategy hash becomes
  `57720813…` — a v5 redeploy is needed to ship it on testnet.
- **Not a bug**: `max_reward_age` is a keeper-SLA knob by design
  (cadence bound, §"the keeper harvests at least every max_reward_age")
  — it is informational in `Policy`, correctly not enforced on-chain.

## Coverage added in this push

- `loss_budget_accumulates_then_reverts_at_cap` — cumulative window
  budget caps repeated keeper losses; the rejecting harvest rolls its
  swap back atomically.
- `loss_budget_resets_after_window` — tracker resets past
  `loss_window_secs` (oracle must be re-seeded: prices age too).
- `mid_harvest_failure_reverts_atomically` — a validating-but-
  overdrawn second swap leg reverts the whole harvest; position and
  balances unchanged.
- `set_policy_*` — real `strategy_registry.wasm` wired as registry:
  within-ceiling applies; over-ceiling, unlisted venue, and non-keeper
  callers all revert.
- `stale_reward_oracle_skips_claim`, `rescue_*` above.

## Not yet audited

- Real Aquarius testnet pool integration (current smoke env uses mocks).
