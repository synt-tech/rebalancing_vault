//! cl_core logic (homed inside cl_aquarius until a second template needs it):
//! oracle reads, valuation, policy validation, fee accrual.
use crate::clients::*;
use crate::storage::*;
use defindex_strategy_core::StrategyError;
use soroban_sdk::{token::TokenClient, Address, Env, Symbol, U256};

const BPS: i128 = 10_000;
const MAX_SWAP_STEPS: u32 = 5;
/// Reflector DEX feed: base USDC, 14 decimals.
pub const ORACLE_SCALE: i128 = 100_000_000_000_000; // 1e14
const TWO_POW_96: u128 = 0x1_0000_0000_0000_0000_0000_0000;

/// Checked arithmetic on externally-derived magnitudes (oracle prices,
/// pool balances/estimates, keeper-supplied amounts): an adversarial or
/// malformed value must produce `UnderflowOverflow`, not a trap.
pub fn cmul(a: i128, b: i128) -> Result<i128, StrategyError> {
    a.checked_mul(b).ok_or(StrategyError::UnderflowOverflow)
}
pub fn cdiv(a: i128, b: i128) -> Result<i128, StrategyError> {
    a.checked_div(b).ok_or(StrategyError::DivisionByZero)
}
pub fn cadd(a: i128, b: i128) -> Result<i128, StrategyError> {
    a.checked_add(b).ok_or(StrategyError::UnderflowOverflow)
}
pub fn csub(a: i128, b: i128) -> Result<i128, StrategyError> {
    a.checked_sub(b).ok_or(StrategyError::UnderflowOverflow)
}
pub fn cmul_u(a: u128, b: u128) -> Result<u128, StrategyError> {
    a.checked_mul(b).ok_or(StrategyError::UnderflowOverflow)
}
pub fn cdiv_u(a: u128, b: u128) -> Result<u128, StrategyError> {
    a.checked_div(b).ok_or(StrategyError::DivisionByZero)
}

/// Fresh oracle price of `asset` in USDC (1e14 scale), or error.
/// Routes through the asset's configured feed/identifier when one was set
/// at init (`OracleOf`/`OracleAssetOf`), else the default `DataKey::Oracle`
/// under `Stellar(asset)`.
pub fn oracle_price(e: &Env, asset: &Address, p: &Policy) -> Result<i128, StrategyError> {
    let oracle: Address =
        get_opt(e, &DataKey::OracleOf(asset.clone())).unwrap_or_else(|| get(e, &DataKey::Oracle));
    let aid: OracleAsset = get_opt(e, &DataKey::OracleAssetOf(asset.clone()))
        .unwrap_or_else(|| OracleAsset::Stellar(asset.clone()));
    let pd = ReflectorClient::new(e, &oracle)
        .try_lastprice(&aid)
        .ok()
        .and_then(|r| r.ok())
        .flatten()
        .ok_or(StrategyError::ExternalError)?;
    let age = e.ledger().timestamp().saturating_sub(pd.timestamp);
    if age > p.max_oracle_age || pd.price <= 0 {
        return Err(StrategyError::ExternalError);
    }
    Ok(pd.price)
}

/// Pool spot implied by sqrt_price_x96, token1 per token0, 1e14 scale.
/// Assumes equal token decimals (XLM/USDC are both 7dp; revisit otherwise).
pub fn spot_price_1e14(e: &Env, sqrt_price_x96: &U256) -> U256 {
    // (sqrtP * 1e7)^2 / 2^192  =  sqrtP^2 * 1e14 / 2^192
    let scaled = sqrt_price_x96.mul(&U256::from_u128(e, 10_000_000));
    let sq = scaled.mul(&scaled);
    sq.div(&U256::from_u128(e, TWO_POW_96).mul(&U256::from_u128(e, TWO_POW_96)))
}

/// Revert if pool spot deviates from the oracle ratio beyond `oracle_band_bps`.
pub fn check_oracle_band(
    e: &Env,
    pool: &ClPoolClient,
    token0: &Address,
    token1: &Address,
    p: &Policy,
) -> Result<(), StrategyError> {
    let slot0 = pool.get_slot0();
    let spot = spot_price_1e14(e, &slot0.sqrt_price_x96)
        .to_u128()
        .unwrap_or(0);
    let p0 = oracle_price(e, token0, p)? as u128;
    let p1 = oracle_price(e, token1, p)? as u128;
    if p1 == 0 {
        return Err(StrategyError::DivisionByZero);
    }
    // Fair units of token1 per unit of token0 = $t0 / $t1 (e.g. pool
    // t0=USDC $1, t1=XLM $0.40 → 2.5 XLM per USDC).
    let ratio = cdiv(cmul(p0 as i128, ORACLE_SCALE)?, p1 as i128)? as u128;
    let band = cdiv_u(cmul_u(ratio, p.oracle_band_bps as u128)?, BPS as u128)?;
    if spot.abs_diff(ratio) > band {
        return Err(StrategyError::InvalidArgument);
    }
    Ok(())
}

/// Clamp the oracle ratio into the strategy's reference price (1e14 scale,
/// token1 per token0). All internal valuation uses this — never raw spot.
pub fn ref_price_1e14(
    e: &Env,
    pool: &ClPoolClient,
    token0: &Address,
    token1: &Address,
    p: &Policy,
) -> Result<i128, StrategyError> {
    check_oracle_band(e, pool, token0, token1, p)?;
    let p0 = oracle_price(e, token0, p)?;
    let p1 = oracle_price(e, token1, p)?;
    if p1 <= 0 {
        return Err(StrategyError::DivisionByZero);
    }
    // Units of token1 per unit of token0: $t0 / $t1.
    cdiv(cmul(p0, ORACLE_SCALE)?, p1)
}

/// Value the stored position + uncollected LP fees + idle, denominated in the
/// vault asset. `asset_idx` is 0 if vault asset == token0, else 1.
/// Uncollected LP fees are netted by `perf_fee_bps`; `fees_owed` totals in the
/// vault asset are excluded; unclaimed rewards are excluded entirely.
/// Spendable balance of `token`: raw balance minus accrued `fees_owed`
/// (owed funds are not strategy capital and must never be spent).
pub fn spendable(e: &Env, token: &Address) -> i128 {
    let me = e.current_contract_address();
    let raw = TokenClient::new(e, token).balance(&me);
    raw.saturating_sub(owed(e, Role::Platform, token))
        .saturating_sub(owed(e, Role::Operator, token))
}

pub fn compute_balance(
    e: &Env,
    pool: &ClPoolClient,
    token0: &Address,
    token1: &Address,
    asset: &Address,
    pos: &Option<Position>,
    p: &Policy,
) -> Result<i128, StrategyError> {
    let me = e.current_contract_address();
    let ref_px = ref_price_1e14(e, pool, token0, token1, p)?;
    let asset_is_0 = *asset == *token0;

    let mut v0: i128 = 0;
    let mut v1: i128 = 0;
    if let Some(pos) = pos {
        if pos.liquidity > 0 {
            let est = pool.estimate_withdraw_position(
                &me,
                &pos.tick_lower,
                &pos.tick_upper,
                &pos.liquidity,
            );
            v0 = cadd(
                v0,
                i128::try_from(est.get(0).unwrap_or(0))
                    .map_err(|_| StrategyError::UnderflowOverflow)?,
            )?;
            v1 = cadd(
                v1,
                i128::try_from(est.get(1).unwrap_or(0))
                    .map_err(|_| StrategyError::UnderflowOverflow)?,
            )?;
            // uncollected LP fees net of the performance fee
            let fees = pool.get_position_fees(&me, &pos.tick_lower, &pos.tick_upper);
            let keep = csub(BPS, p.perf_fee_bps as i128)?;
            let f0 = i128::try_from(fees.get(0).unwrap_or(0))
                .map_err(|_| StrategyError::UnderflowOverflow)?;
            let f1 = i128::try_from(fees.get(1).unwrap_or(0))
                .map_err(|_| StrategyError::UnderflowOverflow)?;
            v0 = cadd(v0, cdiv(cmul(f0, keep)?, BPS)?)?;
            v1 = cadd(v1, cdiv(cmul(f1, keep)?, BPS)?)?;
        }
    }
    // Spendable idle only — fees_owed excluded for BOTH tokens.
    v0 = cadd(v0, spendable(e, token0))?;
    v1 = cadd(v1, spendable(e, token1))?;

    let total_asset = if asset_is_0 {
        cadd(v0, cdiv(cmul(v1, ORACLE_SCALE)?, ref_px)?)?
    } else {
        cadd(cdiv(cmul(v0, ref_px)?, ORACLE_SCALE)?, v1)?
    };
    Ok(total_asset.max(0))
}

/// Accrue a performance fee on `income` (in `token`) into `fees_owed`.
/// Rounding: fee rounds down in depositors' favour; platform share floors,
/// operator takes the remainder so legs sum exactly.
pub fn skim(
    e: &Env,
    token: &Address,
    income: u128,
    source: Symbol,
    p: &Policy,
) -> Result<(), StrategyError> {
    if income == 0 || p.perf_fee_bps == 0 {
        return Ok(());
    }
    let inc = i128::try_from(income).map_err(|_| StrategyError::UnderflowOverflow)?;
    let fee = cdiv(cmul(inc, p.perf_fee_bps as i128)?, BPS)?;
    if fee == 0 {
        return Ok(());
    }
    let platform = cdiv(cmul(fee, p.platform_split_bps as i128)?, BPS)?;
    let operator = csub(fee, platform)?;
    add_owed(e, Role::Platform, token, platform);
    add_owed(e, Role::Operator, token, operator);
    e.events().publish(
        (Symbol::new(e, "FeeAccrued"),),
        (source, token.clone(), income, platform, operator),
    );
    Ok(())
}

pub fn add_owed(e: &Env, role: Role, token: &Address, amt: i128) {
    if amt <= 0 {
        return;
    }
    let k = DataKey::FeesOwed(role, token.clone());
    let cur: i128 = e.storage().instance().get(&k).unwrap_or(0);
    e.storage().instance().set(&k, &cur.saturating_add(amt));
}

pub fn owed(e: &Env, role: Role, token: &Address) -> i128 {
    e.storage()
        .instance()
        .get(&DataKey::FeesOwed(role, token.clone()))
        .unwrap_or(0)
}

/// Validate a keeper-supplied plan against Policy. Executes nothing.
/// Every swap leg's `out_min` is re-derived from the oracle, never trusted
/// from the keeper, and the new range centre must stay inside `tick_cap_bps`
/// of the keeper-provided `ref_tick`.
pub fn validate_plan(
    e: &Env,
    plan: &SwapPlan,
    p: &Policy,
    spacing: i32,
) -> Result<(), StrategyError> {
    if plan.expiry < e.ledger().timestamp() {
        return Err(StrategyError::DeadlineExpired);
    }
    if plan.steps.len() > MAX_SWAP_STEPS {
        return Err(StrategyError::InvalidArgument);
    }
    for i in 0..plan.steps.len() {
        let s = plan.steps.get(i).unwrap();
        if !p.swap_allowed.contains(&s.pool) {
            return Err(StrategyError::NotAuthorized);
        }
        let pool = ClPoolClient::new(e, &s.pool);
        let tokens = pool.get_tokens();
        let in_tok = tokens.get(s.in_idx).ok_or(StrategyError::InvalidArgument)?;
        let out_tok = tokens
            .get(s.out_idx)
            .ok_or(StrategyError::InvalidArgument)?;
        let p_in = oracle_price(e, &in_tok, p)?;
        let p_out = oracle_price(e, &out_tok, p)?;
        if p_in <= 0 || p_out <= 0 {
            return Err(StrategyError::DivisionByZero);
        }
        let in_amt = i128::try_from(s.in_amount).map_err(|_| StrategyError::InvalidArgument)?;
        let out_min = i128::try_from(s.out_min).map_err(|_| StrategyError::InvalidArgument)?;
        let fair_out = cdiv(cmul(in_amt, p_in)?, p_out)?;
        let min_out = cdiv(cmul(fair_out, csub(BPS, p.max_slippage_bps as i128)?)?, BPS)?;
        if out_min < min_out {
            return Err(StrategyError::InvalidArgument);
        }
    }
    let (tl, tu) = (plan.new_range.tick_lower, plan.new_range.tick_upper);
    if tl < tu {
        if tl % spacing != 0 || tu % spacing != 0 {
            return Err(StrategyError::InvalidArgument);
        }
        // Range half-width within policy bounds: (tu-tl)/2 in ticks
        // (tick ≈ ln(1.0001) steps, so |Δtick| ≈ price deviation in bps).
        let half = (tu.checked_sub(tl).ok_or(StrategyError::InvalidArgument)? / 2) as u32;
        if half < p.range_min_bps || half > p.range_max_bps {
            return Err(StrategyError::InvalidArgument);
        }
        let center = tl.checked_add(tu).ok_or(StrategyError::InvalidArgument)? / 2;
        if center
            .checked_sub(plan.ref_tick)
            .ok_or(StrategyError::InvalidArgument)?
            .abs()
            > p.tick_cap_bps as i32
        {
            return Err(StrategyError::InvalidArgument);
        }
    }
    Ok(())
}
