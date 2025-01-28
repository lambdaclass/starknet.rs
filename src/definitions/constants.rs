use super::block_context::{StarknetChainId, StarknetOsConfig};
use crate::transaction::Address;
use cairo_vm::Felt252;
use lazy_static::lazy_static;

use std::collections::HashMap;

pub(crate) const L2_TO_L1_MSG_HEADER_SIZE: usize = 3;
pub(crate) const L1_TO_L2_MSG_HEADER_SIZE: usize = 5;
pub(crate) const CLASS_UPDATE_SIZE: usize = 1;
pub(crate) const CONSUMED_MSG_TO_L2_N_TOPICS: usize = 3;
pub(crate) const LOG_MSG_TO_L1_N_TOPICS: usize = 2;
pub(crate) const N_DEFAULT_TOPICS: usize = 1; // Events have one default topic.
pub(crate) const CONSUMED_MSG_TO_L2_ENCODED_DATA_SIZE: usize =
    (L1_TO_L2_MSG_HEADER_SIZE + 1) - CONSUMED_MSG_TO_L2_N_TOPICS;

pub(crate) const LOG_MSG_TO_L1_ENCODED_DATA_SIZE: usize =
    (L2_TO_L1_MSG_HEADER_SIZE + 1) - LOG_MSG_TO_L1_N_TOPICS;

/// The (empirical) L1 gas cost of each Cairo step.
pub(crate) const N_STEPS_FEE_WEIGHT: f64 = 0.01;

/// The version is considered 0 for L1-Handler transaction hash calculation purposes.
pub(crate) const L1_HANDLER_VERSION: u64 = 0;

lazy_static! {
    // Ratios are taken from the `starknet_instance` CairoLayout object in cairo-lang.
    pub static ref DEFAULT_CAIRO_RESOURCE_FEE_WEIGHTS: HashMap<String, f64> =
        HashMap::from([
            ("n_steps".to_string(), N_STEPS_FEE_WEIGHT),
            ("output_builtin".to_string(), 0.0),
            ("pedersen_builtin".to_string(), N_STEPS_FEE_WEIGHT * 32.0),
            ("range_check_builtin".to_string(), N_STEPS_FEE_WEIGHT * 16.0),
            ("ecdsa_builtin".to_string(), N_STEPS_FEE_WEIGHT * 2048.0),
            ("bitwise_builtin".to_string(), N_STEPS_FEE_WEIGHT * 64.0),
            ("ec_op_builtin".to_string(), N_STEPS_FEE_WEIGHT * 1024.0),
            ("poseidon_builtin".to_string(), N_STEPS_FEE_WEIGHT * 32.0),
            ("segment_arena_builtin".to_string(), N_STEPS_FEE_WEIGHT * 10.0),
            ("keccak_builtin".to_string(), N_STEPS_FEE_WEIGHT * 2048.0), // 2**11
    ]);
    pub static ref DEFAULT_SEQUENCER_ADDRESS: Address = Address(Felt252::from_hex(
        "0x3711666a3506c99c9d78c4d4013409a87a962b7a0880a1c24af9fe193dafc01"
    ).unwrap());

    pub static ref DEFAULT_STARKNET_OS_CONFIG: StarknetOsConfig = StarknetOsConfig {
        chain_id: StarknetChainId::TestNet.to_felt(),
        fee_token_address: crate::definitions::block_context::FeeTokenAddresses {
            // TODO: Replace with STRK token contract address when deployed
            strk_fee_token_address: Address(Felt252::ZERO),
            eth_fee_token_address: Address(Felt252::from_hex(
                "0x4c07059285c2607d528a4c5220ef1f64d8f01273c23cfd9dec68759f61b544"
            ).unwrap()),
        },
    };

pub static ref DECLARE_VERSION: Felt252 = 2.into();
pub static ref TRANSACTION_VERSION: Felt252 = 1.into();
}

pub const DEFAULT_GAS_PRICE: u128 = 100_000_000_000; // 100 * 10**9
pub const DEFAULT_CONTRACT_STORAGE_COMMITMENT_TREE_HEIGHT: u64 = 251;
pub const DEFAULT_GLOBAL_STATE_COMMITMENT_TREE_HEIGHT: u64 = 251;
pub const DEFAULT_INVOKE_TX_MAX_N_STEPS: u64 = 1000000;
pub const DEFAULT_VALIDATE_MAX_N_STEPS: u64 = 1000000;

// Gas Cost.
// From cairo_programs/constants.cairo.
pub const STEP_GAS_COST: u128 = 100;
pub const INITIAL_GAS_COST: u128 = 10_u128.pow(8) * STEP_GAS_COST;

lazy_static! {
    /// Value generated from `get_selector_from_name('constructor')`.
    pub static ref CONSTRUCTOR_ENTRY_POINT_SELECTOR: Felt252 =
        Felt252::from_dec_str("1159040026212278395030414237414753050475174923702621880048416706425641521556").unwrap();
    /// Value generated from `get_selector_from_name('__default__')`.
    pub static ref DEFAULT_ENTRY_POINT_SELECTOR: Felt252 = Felt252::ZERO;
    /// Value generated from `get_selector_from_name('__execute__')`.
    pub static ref EXECUTE_ENTRY_POINT_SELECTOR: Felt252 =
        Felt252::from_dec_str("617075754465154585683856897856256838130216341506379215893724690153393808813").unwrap();
    /// Value generated from `get_selector_from_name('transfer')`.
    pub static ref TRANSFER_ENTRY_POINT_SELECTOR: Felt252 =
        Felt252::from_hex("0x83afd3f4caedc6eebf44246fe54e38c95e3179a5ec9ea81740eca5b482d12e").unwrap();

    /// Value generated from get_selector_from_name('__validate_declare__')
    pub static ref VALIDATE_DECLARE_ENTRY_POINT_SELECTOR: Felt252 =
        Felt252::from_dec_str("1148189391774113786911959041662034419554430000171893651982484995704491697075").unwrap();
    /// Value generated from `get_selector_from_name('__validate_deploy__')`.
    pub static ref VALIDATE_DEPLOY_ENTRY_POINT_SELECTOR: Felt252 =
        Felt252::from_dec_str("1554466106298962091002569854891683800203193677547440645928814916929210362005").unwrap();

    /// Value generated from `get_selector_from_name('Transfer')`.
    pub static ref TRANSFER_EVENT_SELECTOR: Felt252 =
        Felt252::from_dec_str("271746229759260285552388728919865295615886751538523744128730118297934206697").unwrap();

    pub static ref VALIDATE_ENTRY_POINT_SELECTOR: Felt252 =
        Felt252::from_dec_str("626969833899987279399947180575486623810258720106406659648356883742278317941").unwrap();

    pub static ref VALIDATE_RETDATA: Felt252 =
        Felt252::from_dec_str("370462705988").unwrap();

    pub static ref BLOCK_HASH_CONTRACT_ADDRESS: Address = Address(1.into());
}

// Indentation for transactions meant to query and not addressed to the OS.
lazy_static! {
    static ref QUERY_VERSION_BASE: Felt252 =
        Felt252::from_hex("100000000000000000000000000000000").unwrap();
    pub(crate) static ref QUERY_VERSION_0: Felt252 = Into::<Felt252>::into(0) + *QUERY_VERSION_BASE;
    pub(crate) static ref QUERY_VERSION_1: Felt252 = Into::<Felt252>::into(1) + *QUERY_VERSION_BASE;
    pub(crate) static ref QUERY_VERSION_2: Felt252 = Into::<Felt252>::into(2) + *QUERY_VERSION_BASE;
    pub(crate) static ref QUERY_VERSION_3: Felt252 = Into::<Felt252>::into(3) + *QUERY_VERSION_BASE;
}

// Event Limits
pub(crate) const EVENT_MAX_DATA_LENGTH: usize = 300;
pub(crate) const EVENT_MAX_KEYS_LENGTH: usize = 50;
pub(crate) const MAX_N_EMITTED_EVENTS: u64 = 1000;
