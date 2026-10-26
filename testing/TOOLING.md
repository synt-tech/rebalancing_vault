# Ecosystem testing & audit tooling

Surveyed via the workspace MCP integrations (stellar-docs, stellar-raven →
scout/skills) and the local soroban-security DB, 2026-10-26. Ordered by
how actionable each is for this project **today**.

## Directly usable now

| Tool | What | Apply to us |
|---|---|---|
| `cargo-fuzz` + libfuzzer | Mutation fuzzer; every `#[contracttype]` gets `SorobanArbitrary` under `testutils`. Needs nightly + `cargo install --locked cargo-fuzz` | **Highest value.** Targets: `harvest` with arbitrary `Bytes`/decoded `SwapPlan` (must never panic unhandled — always typed `StrategyError`), `deposit`/`withdraw` arbitrary amounts, `balance()` never-panic invariant, `set_policy` arbitrary `Policy` vs ceilings |
| `proptest` + `SorobanArbitrary` | Property tests inside plain `cargo test` | Invariants: `spendable() ≥ 0` always; platform+operator fee legs sum exactly; `balance()` ≥ 0 and never panics; deposit→withdraw round-trip bound |
| `cargo-mutants` | Mutation testing — reports `MISSED` where tests don't notice source mutations | Run after the suite stabilizes; likely catches: swapped in/out idx, band edge cases, fee rounding direction |
| `stellar snapshot create` + `Env::from_ledger_snapshot_file` | **Fork testing** — snapshot real on-chain contracts into tests | Snapshot the live Aquarius XLM/USDC pool (`CBBMQB…`) + Reflector feed → test `cl_aquarius` against the real deployed ABI offline; also an upgrade-rehearsal path |
| `stellar container start local` | Quickstart local network | Faster/resettable alternative to testnet for the auth+multi-contract E2E |
| Test snapshots | Auto-written `test_snapshots/*.json` per test — diffs expose behavior changes | Already generated; commit and diff-review them |
| Resource profiling | `env.cost_estimate().resources()` in tests; `--cost`/`--resources` flags on `stellar contract invoke` | Feeds the pending harvest-path budget profile — mocks already exceed default test limits |
| `soroban-security` MCP (local) | Semantic search over ~950 real Soroban audit findings + reports | Query for AMM/vault/oracle finding patterns before external audit; e.g. `search_vulnerabilities` tags `Logic Bug`, `Rounding`, `Access Control` |

## Static analysis

- **Scout** (CoinFabrik): `cargo install cargo-scout-audit && cargo scout-audit`
  — 20+ Soroban-specific detectors (missing overflow checks, unprotected
  WASM update, unrestricted transfer, unsafe unwrap, unbounded DoS ops),
  SARIF output for CI.
- **OpenZeppelin `soroban-security-detectors-sdk`**: pre-built detectors
  (`auth_missing`, `unchecked_ft_transfer`, improper TTL extension) + a
  framework for custom detectors.

## Formal verification

- **Certora Sunbeam** — verifies deployed **WASM** (not just source);
  properties written as Rust macros (`cvlr_assert!`). Best fit for the
  accounting invariants in `AUDIT.md` (fees_owed never spendable,
  delivered-amount withdrawal, band checks).
- **Komet** (Runtime Verification) — property tests + FV via KWasm
  semantics.

## Monitoring / process

- **OpenZeppelin Monitor** — self-hosted contract monitoring (Stellar
  support) — post-deployment watch on vault/strategy events.
- **SDF Audit Bank** — subsidized audits for SCF-funded protocols
  ($3M+, 43+ audits; OtterSec, Veridise, Runtime Verification,
  CoinFabrik, Certora, Zellic, Code4rena). Relevant if this project goes
  through SCF.
- **Immunefi** — Stellar core bounty up to $250K; OpenZeppelin-Stellar
  contracts up to $25K (not our code, but relevant if a bug turns up in
  a dependency).
- **Audit registry** — `scout.listAudits` indexes 64 reports
  (stellarsecurityportal.com); useful for reading audits of similar
  vaults/strategies pre-engagement.
- **Almanax** — AI security-review tool (listed in the official security
  tools page).

## Recommended order

1. `cargo scout-audit` on `contracts/` — cheapest broad signal.
2. ~~`cargo-fuzz` targets on `harvest`/`deposit`/`withdraw`/`balance`~~
   — done: `fuzz_plan_decode`, `fuzz_plan_structured`,
   `fuzz_deposit_withdraw` in `contracts/cl_aquarius/fuzz/` (TESTING.md).
   Extend with `set_policy` arbitrary `Policy` vs ceilings.
3. ~~Fork test via `stellar snapshot create`~~ — DONE, passing
   end-to-end on a mainnet snapshot with the strategy as compiled WASM;
   per-op `InvocationResources` now printed (redeploy 156M, withdraw
   275M of the 400M tx cap). See `testing/fork/README.md` for the
   regeneration recipe and the dependency-closure checklist.
4. `proptest` invariants once fuzzing surfaces anything.
5. `cargo-mutants` before any external audit.
6. Sunbeam/Komet + Audit Bank only if shipping to mainnet.

**Installed**: `cargo-scout-audit` 0.3.17 (results in `AUDIT.md`,
cl_aquarius at 0 critical findings); nightly Rust `1.101.0-nightly` +
`cargo-fuzz` 0.13.2 with three live targets (results in `TESTING.md` —
fuzz crate notes below). Not installed yet: `cargo-mutants`.

`cl_aquarius/fuzz` setup notes: the crate carries its own `[workspace]`
(cargo-fuzz rejects workspace membership) and its `Cargo.lock` is pinned
to the workspace dependency resolution — a fresh `cargo fuzz` resolution
picks `ed25519-dalek 3.x`/`rand 0.9`, which breaks
`soroban-env-host`'s `ChaCha20Rng` bounds, so the lockfile is kept in git
deliberately. The `cl_aquarius_strategy` lib gained `crate-type =
["cdylib","rlib"]` and a `testutils` feature forwarding
`soroban-sdk/testutils` so `#[contracttype]` types derive
`SorobanArbitrary` for `fuzz_plan_structured`.
