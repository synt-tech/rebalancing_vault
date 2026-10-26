# Deep contract audit — CL rebalancing platform

**Date:** 2026-10-26
**Reviewer:** omp
**Base:** workspace HEAD `793d75b` plus uncommitted changes in
`hackmeridian_rebalancing_vault/contracts/` and
`defindex/apps/contracts/vault/`
**Status:** internal security review, not a third-party audit or formal
verification.

## 1. Executive summary

**Not ready for third-party operators or mainnet deployment.** The current
implementation has one critical phase-2 provenance bypass, seven high-severity
correctness/recovery issues, and several medium/low issues. The current test
suites are also red: `cl_aquarius` has 3 failing tests and the patched DeFindex
vault has 3 failing tests.

The most important findings are:

1. **Optional `OracleRoute` arguments bypass registry provenance.** An operator
   can supply a legitimate default oracle and then override any managed/reward
   asset with an attacker-controlled feed or identifier. This defeats the
   platform's central safety boundary: prices selected by the operator drive
   valuation, swap bounds, loss accounting, and harvest execution.
2. **Policy updates can disable safety mechanisms.** `set_policy` does not run
   the intrinsic policy validator. A manager can set `loss_window_secs = 0`
   (resetting the cumulative loss budget every harvest), `max_oracle_age = 0`
   (brick fresh-price paths), or `max_reward_age = 0` (open harvest to anyone).
3. **Recovery is incomplete.** `platform.rescue_vault` reverts for an open
   position when the pool is in emergency mode, and the newer in-kind rescue
   can move an unsupported foreign token into a single-asset, non-upgradable
   vault with no depositor recovery path.
4. **Registry ceilings are optional and global, not template-bound.** Missing
   ceilings fail open; a global ceiling change can widen policies for existing
   operators, contrary to the per-template/non-retroactive design.
5. **The P3 role patch is not verified.** `set_emergency_manager` now has a
   failing positive-rotation test with `Auth InvalidAction`, while two negative
   `should_panic` tests unexpectedly pass. This needs root-cause analysis before
   shipping the one-sided-freeze change.

Good news: P1/P2 accounting tests pass, platform deployment/binding works in
the current integration tests, the null router fails closed, and Scout reports
zero critical findings in production crates. Those do not offset the issues
below.

## 2. Scope and method

### Reviewed

- `hackmeridian_rebalancing_vault/contracts/cl_aquarius/src/{lib.rs,cl_core.rs,clients.rs,storage.rs,test.rs}`
- `hackmeridian_rebalancing_vault/contracts/{platform,strategy_registry,null_router,mocks}/src/`
- `hackmeridian_rebalancing_vault/contracts/platform/src/test.rs`
- `defindex/apps/contracts/vault/src/{lib.rs,access.rs,strategies.rs,report.rs,router.rs}`
- `defindex/apps/contracts/vault/src/test/vault/{admin.rs,rescue.rs,loss_carry.rs,under_delivery.rs}`
- `platform_design.md` and `testing/AUDIT.md`

Out of scope: off-chain solver/keeper/frontend/indexer, economic calibration,
mainnet deployment state, and formal verification.

### Evidence used

- Manual adversarial review of strategy, registry, platform, and vault patch.
- Independent platform/registry review slice, manually triaged against source.
- `cargo-scout-audit 0.3.17` on the contracts workspace.
- Workspace and DeFindex test runs.
- Temporary reproduction tests for policy bypasses (removed after execution).
- Raven research (direct JSON-RPC after refreshing the local OAuth token):
  Aquarius audit directory, Reflector official docs, and audit-corpus results.

Severity used below: **Critical** = trust-boundary/funds compromise;
**High** = policy bypass, unrecoverable funds, or durable denial of service;
**Medium** = correctness/liveness/user-protection defect with narrower reach;
**Low** = audit-trail or informational defect. Confidence is stated per finding.

## 3. Findings

### F-01 — Critical (phase 2): optional oracle routes bypass registry provenance

**Confidence: high.** `platform.create_vault` forwards `init_args` unchanged
(`platform/src/lib.rs:109-117`). The registry rewrites only `init_args[2]` and
`[3]`, and allowlists only indices `0`, `1`, and `6`
(`strategy_registry/src/lib.rs:216-248`). It does not inspect index 7.

The strategy then accepts `init_args[7]: Vec<OracleRoute>`
(`cl_aquarius/src/lib.rs:120-128`) and `oracle_price` prefers those routes over
the default allowlisted oracle (`cl_core.rs:59-69`). A route contains an
arbitrary `oracle` and `asset_id` (`clients.rs:108-113`).

**Exploitation:** an operator supplies a legitimate default oracle, then maps a
managed or reward token to an operator-controlled oracle, or to a wrong
identifier on an otherwise legitimate feed. The operator also selects the
keeper. Attacker-selected prices then drive:

- `ref_price_1e14` valuation and `compute_balance`;
- `check_oracle_band` acceptance/rejection;
- `validate_plan` fair-output bounds;
- reward conversion and loss-budget accounting.

`max_slippage_bps` does not help when the price reference itself is malicious.

**Fix:** validate every route at deployment. Curate oracle address, permitted
asset, and permitted feed/identifier pair. Restrict route assets to the managed
pool tokens and configured reward token. Reject unknown extra constructor
arguments. Add a rogue-index-7 deployment test and a test proving route use.

---

### F-02 — High: `set_policy` bypasses intrinsic policy validation

**Confidence: high; reproduced.** `validate_policy_bounds` exists only in the
constructor path (`cl_core.rs:37-53`, called at `lib.rs:95`). The update path
(`lib.rs:618-632`) calls only `registry.validate_policy`, whose checks cover a
small set of ceilings and venues (`strategy_registry/src/lib.rs:323-350`).

Consequences:

- `loss_window_secs = 0` resets the loss tracker every harvest, defeating the
  cumulative loss budget.
- `max_oracle_age = 0` makes all fresh-price paths fail, causing DoS.
- `max_slippage_bps > 10_000` or `perf_fee_bps > 10_000` passes registry checks
  and later makes arithmetic/fee paths revert.
- Invalid range/tick settings can also be installed after deployment.

**Evidence:** temporary probe `audit_tmp_set_policy_accepts_zero_oracle_age`
completed successfully against the real registry WASM. The probe was removed
after execution.

**Fix:** call `validate_policy_bounds` on every policy update and extend the
registry/template ceilings with minimum loss-window duration, maximum oracle
age, range/tick bounds, and other security-relevant constraints.

---

### F-03 — High: `max_reward_age` is an unbounded permissionless-harvest switch

**Confidence: high; reproduced.** `harvest` computes
`overdue = last != 0 && now >= last + max_reward_age`
(`cl_aquarius/src/lib.rs:313-337`). When overdue, any authenticated caller may
harvest instead of only the vault RebalanceManager/Manager.

`max_reward_age` is not bounded by `validate_policy_bounds` or the registry.
A manager can set it to zero, after which any caller can harvest immediately
following the first successful harvest.

**Evidence:** temporary probe
`audit_tmp_zero_reward_age_opens_harvest_to_anyone` set `max_reward_age = 0`
through the real registry, then successfully harvested with a random caller.
The probe was removed after execution.

This is more than a liveness feature: a caller can submit a valid plan with
`new_range = (0, 0)`, forcing the strategy to unwind its CL position and stop
earning. Plan validation bounds price/range safety, but does not prevent this
griefing/yield-loss path.

**Fix:** make permissionless harvest an explicit, reviewed phase-2 feature,
not an incidental policy value. Bound `max_reward_age`, reject zero, and decide
whether permissionless callers may unwind without redeploying. If retained,
add a keeper bounty and tests for unauthorized/overdue callers and unwind-only
plans.

---

### F-04 — High: platform rescue fails for an open position in pool emergency mode

**Confidence: high.** `platform.rescue_vault` exposes only vault `rescue`
(`platform/src/lib.rs:216-218`). Vault rescue calls `unwind_from_strategy`
(`defindex/apps/contracts/vault/src/lib.rs:456-468`), which invokes ordinary
`strategy.withdraw` (`strategies.rs:151-155`).

`cl_aquarius::withdraw` rejects when the pool is in emergency mode and a
position is open (`cl_aquarius/src/lib.rs:195-200`). Therefore the platform's
emergency call reverts before unwinding; its pause is rolled back too.

The strategy has `emergency_exit` and in-kind `rescue`
(`cl_aquarius/src/lib.rs:538-614`), but no platform passthrough. The platform
integration test rescues an **empty** strategy (`platform/src/test.rs:298-301`),
so it cannot detect this failure.

**Fix:** add an admin-gated, venue-independent recovery flow using
`emergency_exit` and safe in-kind recovery, with correct invoker authorization
and vault accounting/pause handling. Test with an invested open position and
pool emergency mode enabled.

---

### F-05 — High: in-kind rescue strands the foreign token in single-asset vaults

**Confidence: high.** Platform vaults list exactly one asset
(`platform/src/lib.rs:140-147`). `cl_aquarius::rescue` transfers both pool
tokens to the vault (`cl_aquarius/src/lib.rs:583-612`).

The vault pays withdrawals only for listed assets
(`defindex/apps/contracts/vault/src/lib.rs:335-419`), and router swaps require
both tokens to be listed (`router.rs:28-31`). The vault is non-upgradable
(`lib.rs:819-830`). After a real CL unwind, transferring the foreign leg to the
vault therefore provides no depositor recovery path for that leg: it cannot be
withdrawn, invested, swapped through the vault, or fixed by upgrade.

**Fix:** either keep the foreign leg in strategy custody until conversion is
possible, or implement a two-asset vault/rescue design with pro-rata withdrawal
of both assets. Test recovery of **both** token legs to depositors, not merely
token arrival at the vault.

---

### F-06 — High: ceilings are optional and global rather than template-bound

**Confidence: high.** The design calls for ceilings at template registration.
The implementation stores one global `Ceilings` entry via `set_ceilings`
(`strategy_registry/src/lib.rs:153-155`), while `register_template` stores no
ceilings (`:103-110`).

Two defects follow:

1. **Fail-open configuration:** `check_policy` runs ceiling checks only
   `if let Some(c)` (`:323-337`). If the admin forgets `set_ceilings`, an
   operator can choose `perf_fee_bps = 10000` and `platform_split_bps = 0`,
   assigning all realized income to the operator.
2. **Retroactive widening:** to support a second template with looser limits,
   the admin must loosen global limits. Operators of previously deployed
   strategies can then widen their own policies under those looser limits,
   contrary to per-template and non-retroactive policy guarantees.

`validate_policy` receives no strategy/template identity (`:317-320`).

**Fix:** require ceilings at template registration and fail closed if absent.
Store ceilings per template; snapshot instance limits if later edits should
apply only to new instances. Validate updates against instance/template
identity.

---

### F-07 — High: permissionless deployments can grow registry instance storage into DoS

**Confidence: high.** `deployer.require_auth()` only proves the caller chose
to sign; it does not restrict who may deploy
(`strategy_registry/src/lib.rs:200-209`). Each unique salt creates a permanent
`Deployed(address)` record (`:260-277`). Instances, templates, venues, and
ceilings all use instance storage, not the design's persistent storage with TTL
management (`platform_design.md:347`).

An attacker can deploy many valid strategies with unique salts. The registry's
single instance entry grows until deployment, venue/template writes, or
binding updates hit ledger-entry/resource limits. There is no deletion or
registry upgrade path.

**Fix:** move attacker-cardinality records and template/venue configuration to
persistent storage with explicit TTL bumps. Keep only small globals in instance
storage. Add storage-growth tests and registry migration/runbook notes.

---

### F-08 — High: existing EmergencyManagers cannot rotate through the platform

**Confidence: high.** Every platform vault assigns EmergencyManager to the
platform contract (`platform/src/lib.rs:122-125`). The platform API has no
emergency-manager rotation method (`:95-218`). `registry.set_platform` changes
only future binding authority (`strategy_registry/src/lib.rs:163-165`).

Vault rotation requires authorization from the current EmergencyManager
(`defindex/apps/contracts/vault/src/access.rs:95-97`). A replacement platform
therefore cannot take over existing vaults, and the old platform cannot rotate
through its own API. Loss of the old platform admin or replacement because of
a wrapper defect strands pause/unpause/rescue control.

**Fix:** add an admin-gated `set_emergency_manager` passthrough and test
rotation between two real platform instances against the real vault WASM.

---

### F-09 — Medium: platform does not force `exact_delivery = false`

**Confidence: high.** `platform.create_vault` forwards the constructor vector
unchanged (`platform/src/lib.rs:109-113`), and the registry preserves optional
index 8 (`strategy_registry/src/lib.rs:216-229`). The strategy stores that
Boolean (`cl_aquarius/src/lib.rs:130-134`) and rejects under-delivery when true
(`:292-297`).

An operator can create a platform vault with `init_args[8] = true`. Normal CL
exit costs then make withdrawals revert instead of paying P2's measured
delivered amount. When exact delivery succeeds, exit costs are socialized to
remaining holders rather than borne by the withdrawer. This contradicts
`platform_design.md:433-437`.

**Fix:** normalize index 8 to false in `platform.create_vault` (or reject true
there). The generic registry may retain true for separately deployed
stock-vault strategies. Test true/false/malformed/absent values.

---

### F-10 — Medium: zero-valued asset withdrawals bypass `min_amounts_out`

**Confidence: high.** In the patched vault's `withdraw`
(`defindex/apps/contracts/vault/src/lib.rs:335-419`), the minimum-output check
exists only in the positive-request branch. The zero branch returns zero
without checking the caller's minimum.

In a multi-asset vault, a small withdrawal can round one asset's requested
amount to zero while its supplied minimum is one. The vault burns the shares,
pays zero for that asset, and succeeds despite the explicit floor.

**Fix:** compute payout initially as zero and enforce each asset's minimum in
one common path after the branch. Add a two-asset regression with requested
amount zero and minimum one.

---

### F-11 — Medium: post-withdraw fallback overstates remaining strategy balance

**Confidence: high for the accounting error; medium for reachability.**
`cl_aquarius::withdraw` falls back to `previous - delivered` when the
post-withdraw valuation fails (`cl_aquarius/src/lib.rs:302-306`). The result is
returned as the strategy's remaining balance and becomes vault `prev_balance`
in `unwind_from_strategy` and fee distribution.

A withdrawal can remove 100 units of value but deliver 90 after exit costs.
The fallback then reports `previous - 90`, overstating remaining value by 10.
A later successful report records a phantom additional loss, and the inflated
`LastBalance` can wrongly charge the next harvest against its loss budget.

**Fix:** base the fallback on the already computed pre-withdraw `total -
amount`; at minimum subtract `amount`, not `delivered`. Add a mock that fails
only the second valuation and assert report continuity and the next harvest
baseline.

---

### F-12 — Medium: future-dated oracle prices are accepted as fresh

**Confidence: high.** `oracle_price` computes age with
`e.ledger().timestamp().saturating_sub(pd.timestamp)`
(`cl_core.rs:59-75`). Any future timestamp produces age zero and passes
`max_oracle_age`.

Raven's audit-corpus search corroborates this class: the Reflector OtterSec
report (`https://stellarsecurityportal.com/report/22`) lists
`OS-REF-ADV-01 [med] Lack of Timestamp Sanitization`.

**Fix:** reject `pd.timestamp > now` before computing age, and apply the same
check when accepting fallback prices. Add a future-dated-price test.

---

### F-13 — Medium: fallback prices have no age bound

**Confidence: high.** `oracle_price_fallback` returns the last accepted price
when fresh data is stale/missing (`cl_core.rs:87-100`), but never checks the
stored `PricePoint.timestamp`. `balance()` and exit valuation can therefore use
an arbitrarily old price indefinitely.

The current band check limits how far spot may be from that stale reference,
and `rescue` provides an in-kind fallback. Nevertheless, `max_oracle_age` does
not bound the fallback path, so an oracle outage plus market movement can
produce stale valuation or blocked normal withdrawals.

**Fix:** define an explicit maximum fallback age (or a documented permanent
fallback policy) and test stale-but-usable vs too-old prices. Keep the normal
exit/rescue distinction clear in user-facing docs.

---

### F-14 — Low: public results and events can report requested rather than delivered amounts

**Confidence: high.** `distribute_strategy_fees` caps transfers at delivered
payout but returns the original locked fee
(`defindex/apps/contracts/vault/src/report.rs:196-230`). That value is exposed
and emitted through vault fee-distribution paths. Rebalance discards measured
delivery; rescue emits the requested strategy balance
(`defindex/apps/contracts/vault/src/lib.rs:453-479`, `:870-878`).

A locked fee of 100 that delivers 90 pays receivers 90 but returns/emits 100.
Full under-delivery can report a positive distribution when nobody received
anything. Indexers, composing callers, and emergency audit trails therefore
overstate proceeds.

**Fix:** return and emit actual delivered payout on fee distribution,
rebalance, and rescue. Keep `locked_fee = 0` reset intentional.

---

### F-15 — Low: unpause events permit caller spoofing

**Confidence: high.** `unpause_strategy` authenticates the stored
EmergencyManager but does not bind the `caller` argument to that address before
emitting it (`defindex/apps/contracts/vault/src/lib.rs:530-546`).

An EmergencyManager-authorized call can supply any `caller`, causing the
unpause event to attribute the action to the Manager or another address. This
is an audit-trail defect, not an auth bypass.

**Fix:** bind `caller` to EmergencyManager (for example
`require_any_role(&[RolesDataKey::EmergencyManager], &caller)`) and test
rejection of a spoofed event caller.

## 4. Test and tooling evidence

### 4.1 `hackmeridian_rebalancing_vault/contracts`

Command:

```bash
cargo test --workspace
```

Observed: **25 passed, 3 failed, 1 ignored**. The three failures are stale
expectations after behavior changes, not evidence that those three code paths
are broken:

| Test | Observation | Assessment |
|---|---|---|
| `withdraw_idle_delivers_requested` | `left: 6_000_000_000`, `right: 4_000_000_000` | `withdraw` now returns **remaining strategy balance**, as required by P2 and `unwind_from_strategy`; the test expects delivered amount. Update the assertion. |
| `stale_oracle_blocks_withdraw` | idle withdrawal no longer errors after the oracle ages out | The current fallback intentionally permits idle withdrawal using the last accepted price. Rewrite the test for open-position/stale-fallback semantics and too-old fallback policy (F-13). |
| `set_policy_within_ceiling_applies` | `Error(Contract, #418)` = `NotAuthorized` | `set_policy` is manager-only, but the test calls with the keeper. Use the vault Manager. |

`cargo test -p platform`: **2/2 pass**
(`create_vault_deploys_binds_and_runs_deposit`,
`deploy_rejects_unlisted_pool`).

`cargo test -p strategy_registry`: **0 tests**. This is a material coverage
gap for the provenance/ceiling code in F-01/F-02/F-06/F-07.

### 4.2 DeFindex vault patch

Command:

```bash
cd defindex/apps/contracts
cargo test -p defindex-vault
```

Observed: **109 passed, 3 failed**. P1/P2 paths including loss-carry and
under-delivery tests pass. The failures are in the current P3 role changes:

| Test | Observation | Required action |
|---|---|---|
| `set_new_emergency_manager_by_emergency_manager` | `HostError: Error(Auth, InvalidAction)` | Root-cause before shipping; this is the positive EmergencyManager rotation path and should pass. |
| `set_new_emergency_manager_by_manager` | `#[should_panic]` test did not panic | Review role-gating and the test's auth setup; the current test does not prove Manager cannot rotate EmergencyManager. |
| `unpause_strategy_by_manager` | `#[should_panic]` test did not panic | The test explicitly uses `mock_all_auths`, which also authorizes EmergencyManager. Replace with an auth setup that can actually distinguish roles. |

### 4.3 Embedded WASM freshness

`platform/src/test.rs` embeds prebuilt DeFindex WASMs. The embedded
`defindex_vault.optimized.wasm` timestamp is **2026-10-25 23:35**, while
`vault/src/lib.rs` and `access.rs` were modified on **2026-10-26 08:48**.
Platform integration tests therefore do **not** verify the current P3 source.
Rebuild embedded artifacts before treating those tests as evidence.

### 4.4 `cargo-scout-audit 0.3.17`

| Crate | Critical | Medium | Minor | Enhancement |
|---|---:|---:|---:|---:|
| `cl_aquarius_strategy` | 0 | 36 | 1 | 2 |
| `defindex_strategy_core` | 0 | 0 | 0 | 0 |
| `null_router` | 0 | 0 | 0 | 0 |
| `platform` | 0 | 4 | 0 | 0 |
| `strategy_registry` | 0 | 14 | 0 | 5 |
| `test_mocks` | 16 | 14 | 0 | 17 |

Production crates have zero Scout critical findings. The medium findings are
mostly unsafe `unwrap`, dynamic storage, unbounded operations, and Vec/Map
input warnings; they require manual triage rather than automatic acceptance.
F-01/F-06/F-07 are examples where the manual review is more security-relevant
than the raw count. The `test_mocks` critical findings are fixture-only.

### 4.5 Raven research

Raven was used for external corroboration, not as a substitute for source
review. The mounted tool returned 401; after refreshing the local OAuth token,
the same Raven JSON-RPC endpoint responded successfully.

- `scout.listAudits({ q: "aquarius" })` returned two Aquarius audits:
  - Cantina, **Aquarius AMM**, report 41, published 2025-08-31:
    `https://stellarsecurityportal.com/report/41`
  - Certora, **Aquarius AMM**, report 5, published 2024-12-12:
    `https://stellarsecurityportal.com/report/5`
  `findingsTotal`/`severityCounts` were `null` (not extracted), so no outcome
  was inferred from these rows.
- `stellarDocs.search_docs("Reflector oracle")` located the official provider
  page: `https://developers.stellar.org/docs/data/oracles/oracle-providers#reflector-network`.
- `scout.searchResearch({ source: "audit" })` surfaced the Reflector OtterSec
  report 22, including timestamp-sanitization findings used to corroborate
  F-12: `https://stellarsecurityportal.com/report/22`.

## 5. Confirmed correct behavior

The following were checked and should not be re-flagged without new evidence:

- P1 `gains_or_losses` accumulation and P2 delivered-amount accounting are
  present; their targeted tests pass.
- Registry/treasury pinning at `init_args[2]` and `[3]`, deployer auth,
  platform binding authorization, and one-shot strategy binding are wired.
- `platform.create_vault` passes `vault_fee = 0`, `upgradable = false`,
  `null_router`, and platform EmergencyManager correctly.
- `platform.call_emergency` uses the two-argument vault ABI and invoker auth;
  the platform integration test covers pause/unpause/rescue for an empty
  strategy.
- The null router fails closed; missing router/swap exports cause the vault's
  manager-swap path to revert.
- `validate_plan` now uses current tick and oracle-derived bounds, and swap
  steps check spendable balances before execution.
- `spendable` excludes accrued fee balances; fee legs sum exactly.
- `claim_fees` is permissionless but resolves destinations on-chain; role
  values above 1 are rejected.
- `LastBalance` is seeded on deposit and refreshed after withdrawal; the loss
  budget uses pre-harvest value as its baseline.
- `emergency_exit` does not swap, and `rescue` uses spendable balances only.
- `upgradable = false` is intentional; fixes to existing vaults require
  rescue/new-vault migration, not in-place upgrades.

## 6. Remediation plan

### P0 — before any third-party operator or new vault deployment

1. **Registry provenance:** validate `OracleRoute` address/asset/identifier;
   reject unexpected constructor shapes; force `exact_delivery = false` in the
   platform path.
2. **Policy integrity:** call `validate_policy_bounds` on updates; require
   per-template ceilings; fail closed when ceilings are absent; add bounds for
   loss window, oracle age, `max_reward_age`, range/tick, and slippage/fee
   fields.
3. **Permissionless harvest:** decide explicitly whether it is in scope. Do
   not let `max_reward_age` silently enable it. If retained, constrain
   unwind-only plans and add a bounty/caller test matrix.

### P1 — before production recovery reliance

4. **Recovery:** add platform `emergency_exit`/in-kind recovery passthrough;
   test an invested position in pool emergency mode; implement full recovery
   of both token legs (two-asset design or strategy-side conversion).
5. **EmergencyManager rotation:** add the platform passthrough and test
   rotation between real platform instances.
6. **Registry storage:** move instance records to persistent storage with TTL
   management and add storage-growth tests.

### P1 — vault patch correctness

7. Enforce `min_amounts_out` for zero-rounded assets.
8. Fix post-withdraw fallback to use `total - amount`.
9. Return/emit actual delivered payout on fee distribution/rebalance/rescue;
   bind unpause event caller.
10. Root-cause the positive EmergencyManager rotation auth failure and replace
    invalid `should_panic` role tests with auth setups that can distinguish
    roles.

### P2 — verification and operations

11. Rebuild embedded WASMs before using platform integration tests as P3
    evidence.
12. Add registry unit/integration tests for rogue routes, missing/per-template
    ceilings, unsafe policy updates, `exact_delivery`, storage growth, and
    `max_reward_age`.
13. Fix the three stale `cl_aquarius` tests and add tests for future-dated
    prices, fallback age, zero/one-second loss windows, and overdue harvest.
14. Run real-auth and fork tests for rescue, fee receiver trustlines, AQUA
    min-out failure, and the full deployment path. Keep Scout in CI, but treat
    its output as leads, not proof.

## 7. Residual design risks to retain in the threat model

- Reflector and Aquarius admin/upgrade trust remains. Oracle-band checks do
  not make a compromised feed or pool safe.
- Equal token decimals are assumed in valuation and conversion. This is fine
  for XLM/USDC at 7dp; generalize before adding other pairs.
- Loss budget is measured against harvest-to-harvest balance changes, which
  can include execution-relevant price movement, not only pure swap leak.
- Single-asset vaults still need an explicit, tested answer for foreign-leg
  recovery; `rescue` alone is not that answer.
- Platform Manager can later enable a nonzero vault fee in v0/v1 by design;
  phase-2 `manager_proxy` must close it.
- Normal withdrawals with an open position can still be blocked when the
  oracle is stale and spot has moved outside the stale band. In-kind recovery
  must therefore be fully wired and tested.
- `compute_balance` uses Aquarius's current-spot
  `estimate_withdraw_position`; conversion uses the clamped reference price.
  Re-evaluate share-price manipulation if a nonzero vault fee is ever enabled.

## 8. Audit conclusion

The implementation has a sound high-level architecture and meaningful test
coverage, but the current working tree is not a safe production candidate.
The critical problem is not arithmetic or Soroban mechanics; it is that
**operator-controlled configuration can still select the price reference and
disable policy safeguards**, while recovery and registry lifecycle controls
are incomplete. Fix F-01 through F-08 first, then make the P3 and strategy test
suites green before considering deployment.
