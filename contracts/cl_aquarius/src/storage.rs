use soroban_sdk::{contracttype, Address, Env, Vec};

#[contracttype]
#[derive(Clone, Debug)]
pub struct Policy {
    /// Venue pool addresses swaps may execute on (registry-capped superset).
    pub swap_allowed: Vec<Address>,
    /// Max |pool_spot − oracle| deviation, in bps.
    pub oracle_band_bps: u32,
    /// Max oracle price age in seconds.
    pub max_oracle_age: u64,
    /// Max slippage on any single swap leg, in bps (oracle-referenced).
    pub max_slippage_bps: u32,
    /// Cumulative rebalance loss budget per window, in bps of TVL.
    pub max_loss_bps_per_window: u32,
    pub loss_window_secs: u64,
    /// Min seconds between harvests.
    pub cooldown_secs: u64,
    /// Position half-width bounds, in bps of current price.
    pub range_min_bps: u32,
    pub range_max_bps: u32,
    /// Max |new_center_tick − current_tick| as bps of price.
    pub tick_cap_bps: u32,
    /// Performance fee on realized income, bps.
    pub perf_fee_bps: u32,
    /// Platform share of the performance fee, bps (rest → operator).
    pub platform_split_bps: u32,
    /// Min claimed-reward value (in vault asset) worth converting.
    pub min_reward_swap: i128,
    /// Max age of unclaimed rewards / idle funds — bounds harvest cadence.
    pub max_reward_age: u64,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct Position {
    pub tick_lower: i32,
    pub tick_upper: i32,
    pub liquidity: u128,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct SwapStep {
    pub pool: Address,
    pub in_idx: u32,
    pub out_idx: u32,
    pub in_amount: u128,
    pub out_min: u128,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct SwapPlan {
    pub steps: Vec<SwapStep>,
    pub expiry: u64,
    pub new_range: Option<(i32, i32)>,
}

#[contracttype]
#[derive(Clone, Copy, Debug)]
pub enum Role {
    Platform,
    Operator,
}

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Vault,
    Asset,
    Pool,
    Oracle,
    Registry,
    Treasury,
    /// AQUA (or other reward token) and the CL pool used to convert it
    /// into a managed-pool token. Formula-driven, not part of SwapPlan.
    RewardToken,
    RewardPool,
    Policy,
    Position,
    /// (Role, token) -> i128 accrued fees awaiting claim.
    FeesOwed(Role, Address),
    LastHarvest,
    LastBalance,
    LossTracker,
}

pub fn get<T: soroban_sdk::TryFromVal<Env, soroban_sdk::Val>>(e: &Env, k: &DataKey) -> T {
    e.storage().instance().get(k).unwrap()
}

pub fn get_opt<T: soroban_sdk::TryFromVal<Env, soroban_sdk::Val>>(e: &Env, k: &DataKey) -> Option<T> {
    e.storage().instance().get(k)
}

pub fn set<T: soroban_sdk::IntoVal<Env, soroban_sdk::Val>>(e: &Env, k: &DataKey, v: &T) {
    e.storage().instance().set(k, v);
}
