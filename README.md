# Y.A.R.V. — Automated Rebalancing Vaults

Deposit-and-forget liquidity vaults for Stellar's concentrated-liquidity
pools. **HackMeridian 2026 submission — live on Stellar testnet.**

- **Live app**: https://synt.tech/rebalancing_vault/
- **Deployment record**: [`DEPLOYMENTS.md`](DEPLOYMENTS.md) — every contract
  address, WASM hash and verification tx hash
- **Demo video / pitch deck**: `video/`, `pitch_deck.pptx`

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
  withdraw → swap → deposit bundle.

Doing this badly is expensive: an unguarded rebalance swap can lose more
in one transaction than the position earns in days, and every rebalance
leaks execution cost — a position must earn **more than that leak in fees
between triggers** or rebalancing is negative-EV.

So the vault productizes order management: contract custody + policy
on-chain, strategy off-chain, atomic rebalances.

## What shipped: a CL rebalancing platform on Soroban

A **self-operated CL rebalancing platform**: our own vault factory on
Soroban hosting **public share-token vaults** for Aquarius concentrated
pools — the "deposit-and-forget" LP pattern that made concentrated
liquidity usable for ordinary users on every chain where it shipped:

- A user deposits tokens (zap-supported, single-sided) in one transaction
  and receives a fungible **vault-share token** — a pro-rata claim on the
  managed position plus accrued fees and rewards.
- The strategy contract owns the CL position; a keeper rebalances inside
  on-chain policy bounds; fees and rewards compound into the share price.
- One deposit tx in, one withdraw tx out — the vault absorbs the
  order-management complexity CL introduced.
- **The platform is the product**: the same vault+strategy machinery is
  open to third-party strategists, who earn the operator leg of the
  income fee running a vault on their own rebalancing template.
  `cl_aquarius` is the first/reference strategy.

## Live on testnet

**Try it live:** https://synt.tech/rebalancing_vault/
— connect a testnet wallet (Freighter) to deposit, zap, and watch keeper
rebalances.

Everything below is deployed and transaction-verified — full address and
hash table in [`DEPLOYMENTS.md`](DEPLOYMENTS.md).

### Platform contracts

| Contract | Address | Notes |
|---|---|---|
| DeFindex factory (our instance) | `CDJ467CMUO6R6BLW4PTSMVEGVTUKVNNSIBFLHMK5Q6GK2WMYEDQTKRTR` | stock audited factory, admin = deployer |
| strategy_registry | `CB4LGZVUF345GF5MLMWIZZR2E222SX6DAPQ2DOP3WEKQKBGJRO7I3RES` | template-hash + venue + policy-ceiling governance |
| platform wrapper (v2) | `CDKAKRSZZB2NTOBSTT2KDEZSSLOZSQ4DKTGP6RGSJXFUUY6ZQ2HZLPGF` | pins vaults non-upgradable, `vault_fee = 0`; v1 superseded after a live emergency-ops fix |
| null_router | `CDEYX7WBCCSYXOFBMBOCB2WTLLROPQAWS5YQKUTYSDNBPD23EGASH5LQ` | reverting stub neutralizing the vault's generic swap path |
| patched vault template | `c749c49c…` | installed WASM; deployed per-vault by the factory |
| cl_aquarius template | `522457f9…` | installed + registered in the registry |

### The running vault (v4 — `clUSDC4`)

| Role | Value |
|---|---|
| Vault | `CB77DI4XYWTQTHS5FJWXFFDG5JQJFARP5KKEDM6GGAJ357XC5ZLBFX3V` |
| Bound `cl_aquarius` instance | `CABMCEQGTCR7CVEUCQFHP3MFNMLWYH3WUTERVEAKPEHYNOP6U65GFU76` |
| EmergencyManager | platform contract (re-pointed via `set_emergency_manager`) |
| Fee config | `vault_fee = 0` pinned · non-upgradable · income fee 1500 bps, split 50/50 platform/operator |
| Policy | oracle band 500 bps, max oracle age 600 s, slippage cap 300 bps, loss cap 200 bps/3600 s, cooldown 120 s, range half-width 100–2000 bps, tick cap 1000 bps |

### Verified end-to-end (hashes in DEPLOYMENTS.md)

- `platform.create_vault` → vault + bound strategy, roles pinned
- `vault.deposit` 10e9 USDC → 9,999,999,000 shares (dead-share lock works)
- `vault.rebalance` Invest → strategy pulls 8e9 USDC
- keeper `harvest` → unwind → oracle-validated swap → re-deploy CL
  position at new ticks — verified across **three rebalances**
- `vault.withdraw` all shares → pro-rata unwind through the open position
- **Reward conversion**: claimed AQUA reward → swap → `FeeAccrued` event
  splits the 15% skim platform/operator
- **Web UI zap flows**: single host-op deposit/withdraw bundles via the
  batch router, Freighter-signed — including a 50% USDC / 50%-XLM
  split withdraw

### The demo arc judges can replay

Drift the mock pool's spot from tick 0 → 2300, and the vault position
slides **out of range** (visible in the UI band chart; holdings flip to
100% stable). One keeper `harvest` — tx
[`4bfe5b1e`](https://stellar.expert/explorer/testnet/tx/4bfe5b1eccdab6092068c2e1818c038de161809ee6c9fc5f3e2ff38007a1a1ed)
— unwinds, swaps 7e9 USDC → 13.5e9 XLM inside oracle-derived bounds, and
re-deploys at `[800, 3800]`, back in range. Screenshots in
`video/shots/rebal_05..11`.

### Honest caveat

Venue adapters run against **faithful mocks** of the Aquarius CL pool and
Reflector oracle interfaces (deployed as test fixtures, listed in
DEPLOYMENTS.md) so the full platform wiring — factory → vault → strategy
→ keeper → fee skim — is exercised on a real network with real auth. The
strategy additionally passed a **fork test replaying the real mainnet
pool and Reflector WASMs** from a ledger snapshot, which surfaced (and
fixed) four real bugs. Wiring the adapters to live Aquarius testnet pools
is the next milestone, not a rewrite.

## Architecture

Built on DeFindex's **OtterSec-audited** vault + factory rather than new
custody/share/fee machinery — our new-code surface is deliberately small:

| Contract | Role |
|---|---|
| `contracts/cl_aquarius` | The first CL strategy implementing `DeFindexStrategyTrait`: pairing swaps on deposit/withdraw, oracle-valued `balance()` in USDC, keeper `harvest` (claim fees / convert rewards / re-range via validated SwapPlans), pro-rata `withdraw`, `emergency_exit` |
| `contracts/strategy_registry` | Strategies instantiate only from reviewed WASM template hashes; venues from a curated list; each instance binds to one vault; registry ceilings clamp policy params |
| `contracts/platform` | Creates vaults pinned non-upgradable with `vault_fee = 0`; the only fee is the strategy's capped cut of collected income |
| `contracts/null_router` | Reverting router — neutralizes the vault's generic swap path so strategy swaps only go through listed venues |
| `contracts/mocks` | Faithful Aquarius-pool / Reflector / reward-pool fixtures for testnet exercising and fork testing |

Trust model: the keeper submits plans, but the contract re-validates
everything — venue allowlists, hop limits, plan expiry, cooldown, tick
drift cap, range-width bounds, oracle-referenced slippage (`fair_in`
clamp) and a post-op loss budget. **Every price decision references the
Reflector oracle, never the pool's own spot**, so a keeper or same-ledger
manipulator can't satisfy a plan at a skewed price.

### The DeFindex patch (found while integrating)

Two correctness bugs reproduced and patched, disclosure prepared for
upstream:

- **P1** — `invest_in_strategy` *assigns* a strategy's `gains_or_losses`
  instead of accumulating it, so an `Invest` erases a carried loss and
  the recovery gets fee-locked as profit (repro:
  `defindex/apps/contracts/vault/src/test/vault/loss_carry.rs`).
- **P2** — withdrawals check `min_amounts_out` against the *requested*,
  not *delivered*, amount — fatal for a vault whose exits need a swap.

## Testing & security

- **28 scenario tests** + a real-vault end-to-end (deposit → invest →
  withdraw), oracle staleness/band reverts, auth rejection,
  permissionless claim-fee recipient, emergency unwind.
- **Fork test**: replays the real mainnet Aquarius pool and Reflector
  WASMs from a ledger snapshot (`testing/fork/`) — caught 4 real bugs
  that unit tests hadn't.
- **Static analysis**: `cargo-scout-audit` flagged 37 unchecked-arithmetic
  criticals on first pass; every externally-derived magnitude (oracle
  prices, pool estimates, keeper amounts) now uses checked ops —
  second pass: 0 critical on every production crate (`testing/`).
- **Deep internal audit** (`audit/DEEP_AUDIT_2026-10-26.md`): a second,
  adversarial pass over the platform layer flagged **1 critical + 7 high**
  findings — optional oracle routes can bypass registry provenance,
  `set_policy` skips intrinsic validation, and recovery paths are
  incomplete. Verdict in the doc: not yet ready for third-party
  operators or mainnet — remediation is the top roadmap item, and the
  audit is published here deliberately.

## Prior work & what's new

Disclosed prior work: an offchain rebalancing algorithm we previously
developed, expanded into a basic rebalancing-vault proof-of-concept — the
**synt_dex rebalancer prototype** (self-built, on testnet since
2026-10-02): private contract-owned position, `admin` + `keeper` roles,
keeper plans fully re-validated on-chain (allowlists, expiry, tick drift,
width bounds, `fair_in` clamp, inventory band). It managed a real
XLM/AQUA CL position — including a live catch where its slippage clamp
rejected a route priced ~38% off reference. Its policy engine informed
`cl_aquarius`'s design.

**New during HackMeridian**: the share-token layer (DeFindex vault
integration + the P1/P2 patch), `strategy_registry`/`platform`/
`null_router` contracts, oracle-referencing throughout, zap-style
single-tx UX, the full test suite + fork replay, the web UI, the testnet
deployment, and all presentation materials.

## In this repo

| Path | What it is |
|---|---|
| `contracts/` | `cl_aquarius` strategy, `strategy_registry`, `platform`, `null_router`, `mocks`, `strategy_core` shared crate |
| `testing/` | test fixtures incl. the mainnet fork replay |
| `audit/` | static-analysis + audit notes |
| `web_frontend/` | the vault UI — Vite/React, HashRouter, Stellar Wallets Kit |
| `DEPLOYMENTS.md` | full testnet deployment record — addresses, WASM hashes, tx hashes, ops notes |
| `video/` | demo video (`out.mp4` included; `script.md` + `build.py` reproduce it) |
| `assets/` | screenshots + logos |
| `pitch_deck.pptx` | submission deck (gitignored output; `build_deck.py` + `deck_template.pptx` reproduce it) |

## Roadmap

Post-hackathon, in order (from the pitch deck):

1. **Extensive contract testing** — broader scenario/fuzz coverage on the
   patched vault + strategy templates.
2. **Real-pool wiring** — point venue adapters at live Aquarius testnet
   pools (mocks exercise identical interfaces today).
3. **Sushi V3 adapter** — verify deployed-pool semantics (NFT positions,
   oracle hints, different swap ABI) against the collected interface
   snapshots; second venue per `platform_design` venue-list model.
4. **Strategy template library** — the five rebalancing families observed
   in production CL vaults (re-center, inventory-band, peg-anchored,
   indicator-driven, no-swap reuse) over the shared `strategy_core`
   crate; third-party strategists run templates as operators.
5. **Live mainnet strategy testing + optimisation** on real markets.
6. **Permissionless keeper bounties** on `harvest` so vaults rebalance
   without an ops bot.
7. **SCF application** for continued development.

Open engineering items:

- [ ] Multi-position support (base + one-sided limit) to park leftover
      inventory without swapping.
- [ ] Keeper trigger: subscribe `trade`/`pool_state` events, fire only at
      range exit — vs polling loop.
- [ ] Rewards: `claim_rewards` is exercised on the mock reward pool;
      real-pool gauge config still open.
- [ ] Ship the P1/P2 patch + findings upstream to DeFindex.

## Links

- Live app: https://synt.tech/rebalancing_vault/
- Hackathon: https://www.hackmeridian.com/
- Aquarius CL dev docs: https://docs.aqua.network/developers/concentrated-liquidity
- Sushi on Stellar: https://www.sushi.com/stellar/pool
- Stellar pool tracker: https://stellarindex.io/dexes/sushiswap_v3/
