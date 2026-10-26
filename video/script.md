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
dur: 11
caption: Deposit USDC. The vault does the rest.
voice: clUSDC is a share-token vault on DeFindex. Deposit USDC, and the
vault manages the Aquarius concentrated position for you.

## 3. Architecture
shot: 03_architecture.png
dur: 14
caption: DeFindex custody x Aquarius liquidity x Reflector oracle
voice: It is Stellar Composed end to end — an audited DeFindex vault for
custody and shares, an Aquarius concentrated pool for liquidity, and the
Reflector oracle refereeing every single swap.

## 4. Walkthrough — deposit
shot: 04_deposit.png
dur: 13
caption: One transaction in: USDC in, vault shares out
voice: Here is what using it looks like. Connect, deposit USDC, receive
vault shares — one transaction. No ranges to pick, no bundles to sign.

## 5. Managed position
shot: 05_position.png
dur: 14
caption: Fees claimed, rewards converted, re-ranged on exit
voice: Under the hood, the strategy claims pool fees, converts AQUA
rewards, and re-ranges the position when price exits the band — always
inside on-chain policy limits.

## 6. Walkthrough — withdraw
shot: 06_withdraw.png
dur: 9
caption: One transaction out: pro-rata unwind to USDC
voice: One transaction out. The position unwinds pro-rata and delivers
USDC straight back to the depositor.

## 7. Testnet proof
shot: 07_deployments.png
dur: 13
caption: Live on testnet: contracts, a vault, verified transactions
voice: This is live on testnet today — six contracts, a working vault,
and verified deposit, invest and withdraw transactions, all built this
weekend on an audited foundation.

## 8. Hackathon progress
shot: 08_progress.png
dur: 12
caption: New this weekend: the whole share-token platform layer
voice: We started from a private prototype and shipped the entire
share-token and platform layer — strategy, registry, platform contracts,
tests and deployment — during the hackathon.

## 9. Roadmap
shot: 09_roadmap.png
dur: 10
caption: Next: real pools, strategy templates, third-party strategists
voice: What is next: wiring real pools, a template library of strategy
families, and opening the platform to third-party strategists.
