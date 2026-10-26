# Demo narration script

One section per `##` heading. Fields per section:

- `shot:` — PNG filename in `shots/` (a numbered placeholder is rendered
  if the file is missing; drop the real screenshot in with this name).
- `dur:` — fallback seconds for the section, used only when
  `audio/NN.mp3` does not exist yet.
- `caption:` — one line burned onto the segment (≤ ~90 chars).
- `voice:` — everything after this line is the narration text for
  edge-tts. Keep each section ≤ ~40 words (~15 s).

Rebuild: `./build.py` assembles; `./build.py --tts` (re)generates
`audio/` from this file first.

## 1. Problem
shot: 01_problem.png
dur: 13
caption: CL made Stellar LPs into order managers
voice: Concentrated liquidity made providing liquidity on Stellar feel
like running an orderbook. Drift out of your price band and the position
earns nothing — while holding only the losing token.

## 2. Solution
shot: 02_solution.png
dur: 12
caption: Deposit either pool token. The vault does the rest.
voice: Yarv is a share-token vault on DeFindex. Deposit either pool
token — the vault swaps it into place for you — and a keeper
automatically rebalances the position whenever price drifts out of range.

## 3. Architecture
shot: 03_composed_figure.png
dur: 14
caption: DeFindex custody x Aquarius liquidity x Reflector oracle
voice: It is Stellar Composed end to end — an audited DeFindex vault for
custody and shares, an Aquarius concentrated pool for liquidity, and the
Reflector oracle refereeing every single swap.

## 4. Walkthrough — deposit
shot: 04_deposit.png
dur: 13
caption: One transaction in: either pool token in, vault shares out
voice: Here is what using it looks like. Connect, deposit either pool
token, receive vault shares — one atomic transaction, swap included.
No ranges to pick, no bundles to sign.

## 5. The managed position
shot: rebal_05_baseline_inrange_tick0.png
dur: 10
caption: In range: the band sits around the live price — earning
voice: This is the vault's position on the live pool — the band wrapped
around the current price, both tokens working, in range.

## 6. Price drifts
shot: rebal_06_tick600_drifting.png
dur: 9
caption: Still in range — but the holdings are already drifting
voice: The market starts to move. Still in range, still earning — but
the position mix is already sliding toward one side.

## 7. Out of range
shot: rebal_07_tick1550_oor.png
dur: 10
caption: Price leaves the band — fully flipped, earning nothing
voice: Past the band edge the position flips to all one token and stops
earning — the moment a manual LP would have to act.

## 8. Keeper rebalance
shot: rebal_09_rebalanced_800_3800.png
dur: 13
caption: Keeper proposes; contract unwinds, swaps, re-mints atomically
voice: The keeper proposes a new band. The contract re-checks it against
policy and oracle prices, then unwinds, swaps and re-mints in one atomic
transaction — back in range.

## 9. On-chain receipt
shot: rebal_10_keeper_harvest_tx.png
dur: 9
caption: One keeper call on a public explorer — the whole rebalance
voice: One harvest call, signed and confirmed on testnet — the whole
rebalance is a single transaction on a public explorer.

## 10. Walkthrough — withdraw
shot: 06_withdraw.png
dur: 10
caption: One transaction out: unwind to either or both tokens
voice: One transaction out. The position unwinds and delivers value
straight back — in either or both pool tokens, your pick.

## 11. Testnet proof
shot: 07_deployments.png
dur: 14
caption: Live on testnet: contracts, a vault, verified transactions
voice: This is live on testnet today — six contracts, a working vault,
and verified deposit, harvest and withdraw transactions — plus a fork
test that replays the real mainnet pool and Reflector contracts. All
built this weekend on an audited foundation.

## 12. Hackathon progress
shot: 08_progress.png
dur: 12
caption: New this weekend: the whole share-token platform layer
voice: We started from a private prototype and shipped the entire
share-token and platform layer — strategy, registry, platform contracts,
tests and deployment — during the hackathon.

## 13. Roadmap
shot: 09_roadmap.png
dur: 10
caption: Next: real pools, strategy templates, third-party strategists
voice: What is next: wiring real pools, a template library of strategy
families, and opening the platform to third-party strategists.
