use cairo_vm::Felt252;
use lazy_static::lazy_static;

lazy_static! {
    static ref SECP_SYSCALLS: Vec<String> = vec![
        "secp256k1_add".to_string(),
        "secp256r1_add".to_string(),
        "secp256k1_get_xy".to_string(),
        "secp256r1_get_xy".to_string(),
        "secp256k1_mul".to_string(),
        "secp256r1_mul".to_string(),
        "secp256k1_new".to_string(),
        "secp256r1_new".to_string(),
        "secp256r1_get_point_from_x".to_string(),
        "secp256k1_get_point_from_x".to_string()
    ];
}

pub(crate) const SECP_STEP: u128 = 100;
pub(crate) const SYSCALL_BASE: u128 = 100 * STEP;
pub(crate) const KECCAK_ROUND_COST: u128 = 180000;
pub(crate) const RANGE_CHECK: u128 = 70;
pub(crate) const MEMORY_HOLE: u128 = 10;

pub struct Secp256SyscallHandler {
    points: Vec<Felt252>,
}

impl Secp256SyscallHandler {
    pub fn secp_add(&self) -> Felt252 {
        unimplemented!()
    }
}
