//! null_router — placeholder installed in the `soroswap_router` slot of
//! platform-deployed vaults so the Manager-swap instruction path
//! (`Instruction::SwapExactIn/Out`) always reverts. Real swaps live inside
//! strategies under `Policy` + oracle bounds (platform_design.md §4.2).
#![no_std]
use soroban_sdk::{contract, contractimpl, Env};

#[contract]
pub struct NullRouter;

#[contractimpl]
impl NullRouter {
    pub fn noop(_e: Env) {
        panic!("null router: swaps are not permitted through the vault");
    }
}
