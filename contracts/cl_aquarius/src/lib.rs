//! cl_aquarius — DeFindex-compatible concentrated-liquidity strategy for
//! Aquarius CL pools. Single-position band (v0).
//!
//! Behaviour per platform_design.md:
//! - Bound to exactly one vault (`bind`, one-shot, registry-authorized).
//! - `deposit` pulls the vault asset and parks it as idle; deployment into the
//!   position happens at the next `harvest` (the trait has no `data` param —
//!   plans can only flow through harvest, §5.6).
//! - `withdraw` claims LP fees, skims the performance fee, unwinds a pro-rata
//!   slice of liquidity, converts the foreign leg to the vault asset via the
//!   formula-driven path (pool `swap`, oracle-bounded min-out), and delivers
//!   the actual amount to the vault.
//! - `harvest` is keeper-gated (vault RebalanceManager or Manager), validates
//!   the plan against Policy + oracle, and executes atomically.
//! - Fees accrue in `fees_owed` and are pulled via `claim_fees` — never pushed
//!   on any vault call path (Soroban re-entry + trustline DoS).
//! - Rewards opt-in (`set_rewards_state`) is set at init; `claim` returns the
//!   claimed amount, converted to pool tokens at harvest before the fee cut.
#![no_std]
use soroban_sdk::{contract, contractimpl, token::TokenClient, xdr::FromXdr, Address, Bytes, Env, IntoVal, Symbol, TryFromVal, Vec};

pub mod cl_core;
pub mod clients;
pub mod storage;

use cl_core::*;
use clients::*;
pub use defindex_strategy_core::{event, DeFindexStrategyTrait, StrategyError};
use storage::*;

const STRATEGY_NAME: &str = "cl_aquarius";
const BPS: i128 = 10_000;

#[contract]
pub struct ClAquarius;

fn require_init(e: &Env) -> Result<(Address, Address, Address, Policy), StrategyError> {
    if !e.storage().instance().has(&DataKey::Vault) {
        return Err(StrategyError::NotInitialized);
    }
    Ok((
        get(e, &DataKey::Vault),
        get(e, &DataKey::Asset),
        get(e, &DataKey::Pool),
        get(e, &DataKey::Policy),
    ))
}

fn extend_ttl(e: &Env) {
    e.storage().instance().extend_ttl(100_000, 100_000);
}

/// keeper must be the bound vault's RebalanceManager or Manager.
fn require_keeper(e: &Env, from: &Address, vault: &Address) -> Result<(), StrategyError> {
    from.require_auth();
    let v = VaultClient::new(e, vault);
    let ok = v
        .try_get_rebalance_manager()
        .ok()
        .and_then(|r| r.ok())
        .map(|m| m == *from)
        .unwrap_or(false)
        || v
            .try_get_manager()
            .ok()
            .and_then(|r| r.ok())
            .map(|m| m == *from)
            .unwrap_or(false);
    if !ok {
        return Err(StrategyError::NotAuthorized);
    }
    Ok(())
}

#[contractimpl]
impl DeFindexStrategyTrait for ClAquarius {
    /// init_args: [pool, oracle, registry, treasury, policy, reward_token, reward_pool]
    /// `reward_pool` must pair `reward_token` with one of the managed pool's
    /// tokens (e.g. AQUA/XLM for an XLM/USDC managed pool).
    fn __constructor(e: Env, asset: Address, init_args: Vec<soroban_sdk::Val>) {
        let pool: Address = Address::try_from_val(&e, &init_args.get(0).unwrap()).unwrap();
        let oracle: Address = Address::try_from_val(&e, &init_args.get(1).unwrap()).unwrap();
        let registry: Address = Address::try_from_val(&e, &init_args.get(2).unwrap()).unwrap();
        let treasury: Address = Address::try_from_val(&e, &init_args.get(3).unwrap()).unwrap();
        let policy: Policy = Policy::try_from_val(&e, &init_args.get(4).unwrap()).unwrap();
        let reward_token: Address = Address::try_from_val(&e, &init_args.get(5).unwrap()).unwrap();
        let reward_pool: Address = Address::try_from_val(&e, &init_args.get(6).unwrap()).unwrap();

        // asset must be one of the pool's two tokens
        let tokens = ClPoolClient::new(&e, &pool).get_tokens();
        let t0 = tokens.get(0).unwrap();
        let t1 = tokens.get(1).unwrap();
        if asset != t0 && asset != t1 {
            panic!("asset must be a pool token");
        }
        // reward pool must convert reward_token into t0 or t1
        let rt = ClPoolClient::new(&e, &reward_pool).get_tokens();
        let (r0, r1) = (rt.get(0).unwrap(), rt.get(1).unwrap());
        let pairs_reward = reward_token == r0 || reward_token == r1;
        let yields_pool_tok = r0 == t0 || r0 == t1 || r1 == t0 || r1 == t1;
        if !pairs_reward || !yields_pool_tok {
            panic!("reward_pool must pair reward_token with a managed-pool token");
        }
        set(&e, &DataKey::Asset, &asset);
        set(&e, &DataKey::Pool, &pool);
        set(&e, &DataKey::Oracle, &oracle);
        set(&e, &DataKey::Registry, &registry);
        set(&e, &DataKey::Treasury, &treasury);
        set(&e, &DataKey::Policy, &policy);
        set(&e, &DataKey::RewardToken, &reward_token);
        set(&e, &DataKey::RewardPool, &reward_pool);
        // opt in to pool rewards (live pools require set_rewards_state)
        ClPoolClient::new(&e, &pool).set_rewards_state(&e.current_contract_address(), &true);
    }

    fn asset(e: Env) -> Result<Address, StrategyError> {
        extend_ttl(&e);
        Ok(get(&e, &DataKey::Asset))
    }

    fn deposit(e: Env, amount: i128, from: Address) -> Result<i128, StrategyError> {
        if amount <= 0 {
            return Err(StrategyError::OnlyPositiveAmountAllowed);
        }
        extend_ttl(&e);
        let (vault, asset, pool_addr, policy) = require_init(&e)?;
        if from != vault {
            return Err(StrategyError::NotAuthorized);
        }
        from.require_auth();
        let me = e.current_contract_address();
        TokenClient::new(&e, &asset).transfer(&from, &me, &amount);
        // collect + skim accrued LP fees so deposit pricing is fair
        claim_and_skim_lp_fees(&e, &pool_addr, &policy);
        event::emit_deposit(&e, soroban_sdk::String::from_str(&e, STRATEGY_NAME), amount, from);
        compute_balance_now(&e, &pool_addr, &asset, &policy)
    }

    fn withdraw(e: Env, amount: i128, from: Address, to: Address) -> Result<i128, StrategyError> {
        if amount <= 0 {
            return Err(StrategyError::OnlyPositiveAmountAllowed);
        }
        extend_ttl(&e);
        let (vault, asset, pool_addr, policy) = require_init(&e)?;
        if from != vault || to != vault {
            return Err(StrategyError::NotAuthorized);
        }
        from.require_auth();
        let me = e.current_contract_address();
        let pool = ClPoolClient::new(&e, &pool_addr);
        if pool.get_emergency_mode() {
            return Err(StrategyError::ExternalError);
        }
        let tokens = pool.get_tokens();
        let (t0, t1) = (tokens.get(0).unwrap(), tokens.get(1).unwrap());

        claim_and_skim_lp_fees(&e, &pool_addr, &policy);

        // Unwind a pro-rata slice sized against pre-withdraw total.
        let total = compute_balance_now(&e, &pool_addr, &asset, &policy)?;
        if total <= 0 {
            return Err(StrategyError::InsufficientBalance);
        }
        if let Some(pos) = get_opt::<Position>(&e, &DataKey::Position) {
            if pos.liquidity > 0 {
                let liq_out = (pos.liquidity as i128)
                    .saturating_mul(amount)
                    .saturating_div(total)
                    .min(pos.liquidity as i128) as u128;
                if liq_out > 0 {
                    let est = pool.estimate_withdraw_position(&me, &pos.tick_lower, &pos.tick_upper, &liq_out);
                    let keep = (BPS - policy.max_slippage_bps as i128) as u128;
                    let mins = soroban_sdk::vec![
                        &e,
                        est.get(0).unwrap_or(0) * keep / BPS as u128,
                        est.get(1).unwrap_or(0) * keep / BPS as u128
                    ];
                    pool.withdraw_position(&me, &pos.tick_lower, &pos.tick_upper, &liq_out, &mins);
                    let mut p = pos.clone();
                    p.liquidity -= liq_out;
                    if p.liquidity == 0 {
                        e.storage().instance().remove(&DataKey::Position);
                    } else {
                        set(&e, &DataKey::Position, &p);
                    }
                }
            }
        }

        // Convert enough of the foreign leg to cover `amount`, at ref price.
        let ref_px = ref_price_1e14(&e, &pool, &get(&e, &DataKey::Oracle), &t0, &t1, &policy)?;
        let asset_is_0 = asset == t0;
        let (foreign, asset_tok) = if asset_is_0 { (&t1, &t0) } else { (&t0, &t1) };
        let idle_asset = spendable(&e, asset_tok);
        let mut delivered = idle_asset.min(amount);
        if delivered < amount {
            let shortfall = amount - delivered;
            // foreign input needed: shortfall in asset units → foreign units
            let needed_foreign = if asset_is_0 {
                shortfall * cl_core::ORACLE_SCALE / ref_px
            } else {
                shortfall * ref_px / cl_core::ORACLE_SCALE
            };
            let avail_foreign = spendable(&e, foreign);
            let swap_in = needed_foreign.min(avail_foreign);
            if swap_in > 0 {
                let out_min = (if asset_is_0 {
                    swap_in * ref_px / cl_core::ORACLE_SCALE
                } else {
                    swap_in * cl_core::ORACLE_SCALE / ref_px
                }) * (BPS - policy.max_slippage_bps as i128) / BPS;
                let (in_idx, out_idx) = if asset_is_0 { (1u32, 0u32) } else { (0u32, 1u32) };
                authorize_pull(&e, foreign, &pool_addr, swap_in);
                let got = pool.swap(&me, &in_idx, &out_idx, &(swap_in as u128), &(out_min as u128));
                delivered += got as i128;
            }
        }
        delivered = delivered.min(amount);
        if delivered <= 0 {
            return Err(StrategyError::InsufficientBalance);
        }
        TokenClient::new(&e, &asset).transfer(&me, &to, &delivered);
        event::emit_withdraw(&e, soroban_sdk::String::from_str(&e, STRATEGY_NAME), delivered, to);
        Ok(delivered)
    }

    fn harvest(e: Env, from: Address, data: Option<Bytes>) -> Result<(), StrategyError> {
        extend_ttl(&e);
        let (vault, asset, pool_addr, policy) = require_init(&e)?;
        require_keeper(&e, &from, &vault)?;

        let now = e.ledger().timestamp();
        let last: u64 = e.storage().instance().get(&DataKey::LastHarvest).unwrap_or(0);
        if last != 0 && now < last + policy.cooldown_secs {
            return Err(StrategyError::NotAuthorized);
        }

        let me = e.current_contract_address();
        let pool = ClPoolClient::new(&e, &pool_addr);
        if pool.get_emergency_mode() {
            return Err(StrategyError::ExternalError);
        }
        let tokens = pool.get_tokens();
        let (t0, t1) = (tokens.get(0).unwrap(), tokens.get(1).unwrap());
        let oracle: Address = get(&e, &DataKey::Oracle);
        check_oracle_band(&e, &pool, &oracle, &t0, &t1, &policy)?;

        // 1. Claim + convert rewards → pool token, fee cut from swap output.
        claim_and_convert_rewards(&e, &pool_addr, &asset, &policy)?;
        // 2. LP fees claim + skim.
        claim_and_skim_lp_fees(&e, &pool_addr, &policy);

        // 3. Decode + validate plan, execute steps on allowed venues.
        let plan: SwapPlan = match &data {
            Some(b) if !b.is_empty() => {
                SwapPlan::from_xdr(&e, b).map_err(|_| StrategyError::InvalidArgument)?
            }
            _ => return Err(StrategyError::InvalidArgument),
        };
        validate_plan(&e, &plan, &policy, pool.get_tick_spacing())?;

        // unwind current position (fees already skimmed)
        let mut all: Vec<u128> = Vec::new(&e);
        all.push_back(0);
        all.push_back(0);
        if let Some(pos) = get_opt::<Position>(&e, &DataKey::Position) {
            if pos.liquidity > 0 {
                all = pool.withdraw_position(&me, &pos.tick_lower, &pos.tick_upper, &pos.liquidity, &soroban_sdk::vec![&e, 0, 0]);
                e.storage().instance().remove(&DataKey::Position);
            }
        }
        let _ = all;

        // execute swap steps
        for i in 0..plan.steps.len() {
            let s = plan.steps.get(i).unwrap();
            let sp = ClPoolClient::new(&e, &s.pool);
            let sp_tokens = sp.get_tokens();
            authorize_pull(
                &e,
                &sp_tokens.get(s.in_idx).unwrap(),
                &s.pool,
                s.in_amount as i128,
            );
            sp.swap(&me, &s.in_idx, &s.out_idx, &s.in_amount, &s.out_min);
        }

        // deploy into new range
        if plan.new_range.tick_lower < plan.new_range.tick_upper {
            let (tl, tu) = (plan.new_range.tick_lower, plan.new_range.tick_upper);
            let b0 = spendable(&e, &t0).max(0);
            let b1 = spendable(&e, &t1).max(0);
            if b0 > 0 || b1 > 0 {
                authorize_pull(&e, &t0, &pool_addr, b0);
                authorize_pull(&e, &t1, &pool_addr, b1);
                let (used, liq) = pool.deposit_position(
                    &me,
                    &tl,
                    &tu,
                    &soroban_sdk::vec![&e, b0 as u128, b1 as u128],
                    &0u128,
                );
                let _ = used;
                set(&e, &DataKey::Position, &Position { tick_lower: tl, tick_upper: tu, liquidity: liq });
            }
        }

        // post-check: spot still inside oracle band + loss budget
        check_oracle_band(&e, &pool, &oracle, &t0, &t1, &policy)?;
        let new_bal = compute_balance_now(&e, &pool_addr, &asset, &policy)?;
        check_loss_budget(&e, new_bal, &policy, now)?;

        set(&e, &DataKey::LastHarvest, &now);
        set(&e, &DataKey::LastBalance, &new_bal);
        e.events().publish(
            (Symbol::new(&e, "Rebalance"),),
            (from.clone(), plan.steps.len(), plan.new_range.tick_lower, plan.new_range.tick_upper),
        );
        event::emit_harvest(&e, soroban_sdk::String::from_str(&e, STRATEGY_NAME), 0i128, from, 0i128);
        Ok(())
    }

    fn balance(e: Env, from: Address) -> Result<i128, StrategyError> {
        extend_ttl(&e);
        let (vault, asset, pool_addr, policy) = require_init(&e)?;
        if from != vault {
            return Ok(0);
        }
        match compute_balance_now(&e, &pool_addr, &asset, &policy) {
            Ok(b) => {
                set(&e, &DataKey::LastBalance, &b);
                Ok(b)
            }
            // balance() must not panic — fall back to last computed value on
            // transient oracle failure.
            Err(_) => Ok(e.storage().instance().get(&DataKey::LastBalance).unwrap_or(0)),
        }
    }
}

#[contractimpl]
impl ClAquarius {
    /// One-shot vault binding. Caller must be the registry (or platform ops).
    pub fn bind(e: Env, caller: Address, vault: Address) -> Result<(), StrategyError> {
        extend_ttl(&e);
        caller.require_auth();
        let registry: Address = get(&e, &DataKey::Registry);
        if caller != registry {
            return Err(StrategyError::NotAuthorized);
        }
        if e.storage().instance().has(&DataKey::Vault) {
            return Err(StrategyError::InvalidArgument);
        }
        set(&e, &DataKey::Vault, &vault);
        Ok(())
    }

    /// Pull payout of accrued fees. `role` 0=Platform, 1=Operator.
    /// Permissionless trigger: recipients are resolved at payout time
    /// (platform → registry treasury; operator → vault fee receiver).
    pub fn claim_fees(e: Env, caller: Address, role: u32, token: Address) -> Result<i128, StrategyError> {
        extend_ttl(&e);
        let _ = caller;
        let (vault, ..) = require_init(&e)?;
        let registry: Address = get(&e, &DataKey::Registry);
        let (r, dest) = if role == 0 {
            (Role::Platform, RegistryClient::new(&e, &registry).treasury())
        } else {
            (Role::Operator, VaultClient::new(&e, &vault).get_fee_receiver())
        };
        let k = DataKey::FeesOwed(r, token.clone());
        let amt: i128 = e.storage().instance().get(&k).unwrap_or(0);
        if amt <= 0 {
            return Ok(0);
        }
        e.storage().instance().set(&k, &0i128);
        TokenClient::new(&e, &token).transfer(&e.current_contract_address(), &dest, &amt);
        e.events().publish(
            (Symbol::new(&e, "FeePaid"),),
            (role, dest, token, amt),
        );
        Ok(amt)
    }

    /// Manager/EmergencyManager-triggered exit: unwind the position to strategy
    /// custody with NO swap, claimable later. Vault-callable safe.
    pub fn emergency_exit(e: Env, caller: Address) -> Result<(), StrategyError> {
        extend_ttl(&e);
        let (vault, _, pool_addr, policy) = require_init(&e)?;
        caller.require_auth();
        let v = VaultClient::new(&e, &vault);
        let ok = v.try_get_emergency_manager().ok().and_then(|r| r.ok()).map(|m| m == caller).unwrap_or(false)
            || v.try_get_manager().ok().and_then(|r| r.ok()).map(|m| m == caller).unwrap_or(false);
        if !ok {
            return Err(StrategyError::NotAuthorized);
        }
        claim_and_skim_lp_fees(&e, &pool_addr, &policy);
        let pool = ClPoolClient::new(&e, &pool_addr);
        let me = e.current_contract_address();
        if let Some(pos) = get_opt::<Position>(&e, &DataKey::Position) {
            if pos.liquidity > 0 {
                pool.withdraw_position(&me, &pos.tick_lower, &pos.tick_upper, &pos.liquidity, &soroban_sdk::vec![&e, 0, 0]);
                e.storage().instance().remove(&DataKey::Position);
            }
        }
        Ok(())
    }

    /// Update Policy within registry ceilings (keeper/manager only).
    pub fn set_policy(e: Env, caller: Address, policy: Policy) -> Result<(), StrategyError> {
        extend_ttl(&e);
        let (vault, ..) = require_init(&e)?;
        caller.require_auth();
        let v = VaultClient::new(&e, &vault);
        let ok = v.try_get_rebalance_manager().ok().and_then(|r| r.ok()).map(|m| m == caller).unwrap_or(false)
            || v.try_get_manager().ok().and_then(|r| r.ok()).map(|m| m == caller).unwrap_or(false);
        if !ok {
            return Err(StrategyError::NotAuthorized);
        }
        let registry: Address = get(&e, &DataKey::Registry);
        RegistryClient::new(&e, &registry).validate_policy(&policy);
        set(&e, &DataKey::Policy, &policy);
        Ok(())
    }
}

/// Claim the current position's LP fees and skim `perf_fee_bps` into
/// `fees_owed`. No-op with no position.
/// Pre-authorize a downstream `token.transfer(me -> callee, amount)` so a
/// callee contract (pool/router) can pull tokens on our behalf. Soroban
/// invoker auth only covers the *direct* callee's `require_auth`; nested
/// pulls by that callee must be authorized explicitly — same pattern the
/// DeFindex vault uses for strategy deposits.
fn authorize_pull(e: &Env, token: &Address, callee: &Address, amount: i128) {
    if amount <= 0 {
        return;
    }
    e.authorize_as_current_contract(soroban_sdk::vec![
        &e,
        soroban_sdk::auth::InvokerContractAuthEntry::Contract(
            soroban_sdk::auth::SubContractInvocation {
                context: soroban_sdk::auth::ContractContext {
                    contract: token.clone(),
                    fn_name: Symbol::new(e, "transfer"),
                    args: (
                        e.current_contract_address(),
                        callee.clone(),
                        amount,
                    )
                        .into_val(e),
                },
                sub_invocations: soroban_sdk::vec![&e],
            },
        ),
    ]);
}

fn claim_and_skim_lp_fees(e: &Env, pool_addr: &Address, policy: &Policy) {
    if let Some(pos) = get_opt::<Position>(e, &DataKey::Position) {
        if pos.liquidity > 0 {
            let pool = ClPoolClient::new(e, pool_addr);
            let me = e.current_contract_address();
            let got = pool.claim_position_fees(&me, &pos.tick_lower, &pos.tick_upper);
            if let Some(tokens) = pool.try_get_tokens().ok().and_then(|r| r.ok()) {
                let source = Symbol::new(e, "lp_fees");
                skim(e, &tokens.get(0).unwrap(), got.get(0).unwrap_or(0), source.clone(), policy).ok();
                skim(e, &tokens.get(1).unwrap(), got.get(1).unwrap_or(0), source, policy).ok();
            }
        }
    }
}

/// Claim pool rewards (e.g. AQUA) and convert 100% into the managed-pool
/// token it pairs against in `reward_pool`, cutting `perf_fee_bps` from the
/// swap output. Skips conversion below `min_reward_swap` (claimed tokens
/// stay as unvalued dust — still excluded from `balance()`, consistent with
/// the reward exclusion rule).
fn claim_and_convert_rewards(e: &Env, pool_addr: &Address, asset: &Address, policy: &Policy) -> Result<(), StrategyError> {
    let pool = ClPoolClient::new(e, pool_addr);
    let me = e.current_contract_address();
    if pool.get_user_reward(&me) == 0 {
        return Ok(());
    }
    let claimed = pool.claim(&me) as i128; // returns claimed amount
    if claimed <= 0 {
        return Ok(());
    }
    let reward_tok: Address = get(e, &DataKey::RewardToken);
    let reward_pool_addr: Address = get(e, &DataKey::RewardPool);
    let oracle: Address = get(e, &DataKey::Oracle);

    // Dust gate: skip if claimed value < min_reward_swap (in asset units).
    let p_rew = oracle_price(e, &oracle, &reward_tok, policy)?;
    let p_asset = oracle_price(e, &oracle, asset, policy)?;
    let value_asset = claimed * p_rew / p_asset;
    if value_asset < policy.min_reward_swap {
        return Ok(());
    }

    let rp = ClPoolClient::new(e, &reward_pool_addr);
    let rt = rp.get_tokens();
    let (in_idx, out_idx, out_tok) = if rt.get(0).unwrap() == reward_tok {
        (0u32, 1u32, rt.get(1).unwrap())
    } else {
        (1u32, 0u32, rt.get(0).unwrap())
    };
    let p_out = oracle_price(e, &oracle, &out_tok, policy)?;
    // oracle-derived min-out (equal token decimals assumed, see balance())
    let fair_out = claimed * p_rew / p_out;
    let out_min = fair_out * (BPS - policy.max_slippage_bps as i128) / BPS;
    authorize_pull(e, &reward_tok, &reward_pool_addr, claimed);
    let got = rp.swap(&me, &in_idx, &out_idx, &(claimed as u128), &(out_min as u128)) as i128;
    skim(e, &out_tok, got as u128, Symbol::new(e, "rewards"), policy)
}

fn compute_balance_now(e: &Env, pool_addr: &Address, asset: &Address, policy: &Policy) -> Result<i128, StrategyError> {
    let pool = ClPoolClient::new(e, pool_addr);
    let tokens = pool.get_tokens();
    let pos: Option<Position> = get_opt(e, &DataKey::Position);
    compute_balance(
        e,
        &pool,
        &get(e, &DataKey::Oracle),
        &tokens.get(0).unwrap(),
        &tokens.get(1).unwrap(),
        asset,
        &pos,
        policy,
    )
}

/// Track cumulative rebalance loss vs the loss window budget.
/// LossTracker = (window_start, loss_bps_used).
fn check_loss_budget(e: &Env, new_bal: i128, policy: &Policy, now: u64) -> Result<(), StrategyError> {
    let prev: i128 = e.storage().instance().get(&DataKey::LastBalance).unwrap_or(new_bal);
    if new_bal >= prev || prev <= 0 {
        return Ok(());
    }
    let loss_bps = (prev - new_bal) * BPS / prev;
    let (w_start, used): (u64, i128) = e
        .storage()
        .instance()
        .get(&DataKey::LossTracker)
        .unwrap_or((now, 0));
    let (ws, mut u) = if now >= w_start + policy.loss_window_secs { (now, 0) } else { (w_start, used) };
    u += loss_bps;
    if u > policy.max_loss_bps_per_window as i128 {
        return Err(StrategyError::NotAuthorized);
    }
    e.storage().instance().set(&DataKey::LossTracker, &(ws, u));
    Ok(())
}

#[cfg(test)]
mod test;
