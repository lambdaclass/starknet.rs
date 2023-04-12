#![deny(warnings)]

mod cached_state;
mod starknet_state;
mod types;
mod utils;

use self::{
    cached_state::PyCachedState,
    types::{
        block_info::PyBlockInfo, call_info::PyCallInfo, contract_class::PyContractClass,
        contract_entry_point::PyContractEntryPoint, execution_resources::PyExecutionResources,
        ordered_event::PyOrderedEvent, ordered_l2_to_l1_message::PyOrderedL2ToL1Message,
    },
};
use crate::{
    types::{
        general_config::build_general_config,
        starknet_message_to_l1::PyStarknetMessageToL1,
        transaction::{PyTransaction, PyTransactionType},
        transaction_execution_info::PyTransactionExecutionInfo,
        transactions::{
            declare::PyInternalDeclare, deploy::PyInternalDeploy,
            deploy_account::PyInternalDeployAccount, invoke_function::PyInternalInvokeFunction,
        },
    },
    utils::{
        py_calculate_contract_address, py_calculate_contract_address_from_hash,
        py_calculate_event_hash, py_calculate_tx_fee, py_compute_class_hash,
        py_validate_contract_deployed,
        transaction_hash::{
            py_calculate_declare_transaction_hash, py_calculate_deploy_transaction_hash,
            py_calculate_transaction_hash_common, PyTransactionHashPrefix,
        },
    },
};
use cairo_felt::{felt_str, Felt252};
use pyo3::prelude::*;
use starknet_rs::{
    business_logic::state::cached_state::UNINITIALIZED_CLASS_HASH,
    definitions::constants::{
        DEFAULT_CONTRACT_STORAGE_COMMITMENT_TREE_HEIGHT, DEFAULT_GAS_PRICE,
        DEFAULT_SEQUENCER_ADDRESS, DEFAULT_VALIDATE_MAX_N_STEPS, TRANSACTION_VERSION,
    },
    services::api::contract_class::ContractClass,
};
use starknet_state::PyStarknetState;
use std::ops::Shl;
use types::general_config::{PyStarknetChainId, PyStarknetGeneralConfig, PyStarknetOsConfig};

#[cfg(all(feature = "extension-module", feature = "embedded-python"))]
compile_error!("\"extension-module\" is incompatible with \"embedded-python\" as it inhibits linking with cpython");

#[pymodule]
pub fn starknet_rs_py(py: Python, m: &PyModule) -> PyResult<()> {
    eprintln!("WARN: using starknet_rs_py");

    // ~~~~~~~~~~~~~~~~~~~~
    //  Exported Classes
    // ~~~~~~~~~~~~~~~~~~~~

    m.add_class::<PyStarknetState>()?;
    m.add_class::<PyBlockInfo>()?;
    m.add_class::<PyCachedState>()?;
    m.add_class::<PyStarknetGeneralConfig>()?;
    m.add_class::<PyStarknetOsConfig>()?;
    m.add_class::<PyStarknetChainId>()?;
    m.add_class::<PyContractClass>()?;
    m.add_class::<PyContractEntryPoint>()?;
    m.add_class::<PyExecutionResources>()?;
    m.add_class::<PyOrderedEvent>()?;
    m.add_class::<PyOrderedL2ToL1Message>()?;
    m.add_class::<PyCallInfo>()?;

    //  starkware.starknet.core.os.transaction_hash.transaction_hash
    // m.add_class::<PyTransactionHashPrefix>()?;
    // m.add_function(calculate_declare_transaction_hash)?;
    // m.add_function(calculate_deploy_transaction_hash)?;
    // m.add_function(calculate_transaction_hash_common)?;

    // m.add_function(build_general_config)?;

    //  starkware.starknet.public.abi
    // m.add_function(get_selector_from_name)?;

    // m.add_class::<PyAbiEntryType>()?;
    // m.add_function(get_storage_var_address)?;

    //  starkware.starknet.testing.starknet
    // m.add_class::<PyStarknet>()?;
    // m.add_class::<PyStarknetCallInfo>()?;

    //  starkware.starknet.testing.state

    //  starkware.starknet.core.os.contract_address.contract_address
    // m.add_function(calculate_contract_address_from_hash)?;
    // m.add_function(calculate_contract_address)?;

    //  starkware.starknet.core.os.block_hash.block_hash
    // m.add_function(calculate_block_hash)?;
    // m.add_function(calculate_event_hash)?;

    //  starkware.starknet.definitions.error_codes
    // m.add_class::<PyStarknetErrorCode>()?;

    //  starkware.starknet.services.api.feeder_gateway.response_objects
    // m.add_class::<PyBlockIdentifier>()?; this one is a Python Union
    // m.add_class::<PyBlockStateUpdate>()?;
    // m.add_class::<PyBlockStatus>()?;
    // m.add_class::<PyBlockTransactionTraces>()?;
    // m.add_class::<PyTransactionSimulationInfo>()?;
    // m.add_class::<PyStarknetBlock>()?;
    // m.add_class::<PyTransactionInfo>()?;
    // m.add_class::<PyTransactionReceipt>()?;
    // m.add_class::<PyTransactionStatus>()?;
    // m.add_class::<PyTransactionTrace>()?;
    // m.add_class::<PyTransactionExecution>()?;
    // m.add_class::<PyTransactionSpecificInfo>()?;
    // m.add_class::<PyFeeEstimationInfo>()?;
    // m.add_class::<PyDeployedContract>()?;
    // m.add_class::<PyStateDiff>()?;
    // m.add_class::<PyStorageEntry>()?;
    // m.add_class::<PyEvent>()?;
    // m.add_class::<PyFunctionInvocation>()?;
    // m.add_class::<PyL2ToL1Message>()?;
    // m.add_class::<PyDeclareSpecificInfo>()?;
    // m.add_class::<PyDeployAccountSpecificInfo>()?;
    // m.add_class::<PyDeploySpecificInfo>()?;
    // m.add_class::<PyInvokeSpecificInfo>()?;
    // m.add_class::<PyL1HandlerSpecificInfo>()?;

    //  starkware.starknet.business_logic.execution.execute_entry_point
    // m.add_class::<PyExecuteEntryPoint>()?;

    //  starkware.starknet.business_logic.execution.objects
    // m.add_class::<PyTransactionExecutionContext>()?;
    // m.add_class::<PyResourcesMapping>()?;

    //  starkware.starknet.business_logic.fact_state.state
    // m.add_class::<PyExecutionResourcesManager>()?;

    //  starkware.starknet.business_logic.state.state_api
    // m.add_class::<PySyncState>()?;
    // m.add_class::<PyStateReader>()?;

    //  starkware.starknet.business_logic.utils
    // m.add_function(validate_contract_deployed)?;
    // m.add_function(verify_version)?;

    //  starkware.starknet.core.os.class_hash
    // m.add_function(get_contract_class_struct)?;
    // m.add_function(load_program)?;
    // m.add_function(compute_class_hash)?;

    //  starkware.starknet.core.os.os_utils
    // m.add_function(prepare_os_context)?;
    // m.add_function(validate_and_process_os_context)?;

    //  starkware.starknet.core.os.segment_utils
    // m.add_function(get_os_segment_ptr_range)?;
    // m.add_function(validate_segment_pointers)?;

    //  starkware.starknet.core.os.syscall_utils
    // m.add_class::<PyDeprecatedBLSyscallHandler>()?;
    // m.add_class::<PyHandlerException>()?;

    //  starkware.starknet.services.api.feeder_gateway.feeder_gateway_client
    // m.add_class::<PyFeederGatewayClient>()?;

    //  starkware.starknet.services.api.gateway.transaction
    // m.add_class::<PyAccountTransaction>()?;
    // m.add_class::<PyDeclare>()?;
    // m.add_class::<PyDeployAccount>()?;
    // m.add_class::<PyInvokeFunction>()?;
    // m.add_class::<PyDeploy>()?;
    // m.add_class::<PyTransaction>()?;

    m.add_class::<PyTransactionHashPrefix>()?;
    m.add_class::<PyTransaction>()?;
    m.add_class::<PyTransactionType>()?;
    m.add_class::<PyStarknetMessageToL1>()?;
    m.add_class::<PyTransactionExecutionInfo>()?;
    m.add_class::<PyInternalDeclare>()?;
    m.add_class::<PyInternalDeploy>()?;
    m.add_class::<PyInternalDeployAccount>()?;
    m.add_class::<PyInternalInvokeFunction>()?;

    //  starkware.starknet.business_logic.transaction.objects
    // m.add_class::<PyInternalL1Handler>()?;  // isn't implemented

    // ~~~~~~~~~~~~~~~~~~~~
    //  Exported Functions
    // ~~~~~~~~~~~~~~~~~~~~

    m.add_function(wrap_pyfunction!(build_general_config, m)?)?;
    m.add_function(wrap_pyfunction!(py_calculate_transaction_hash_common, m)?)?;
    m.add_function(wrap_pyfunction!(py_calculate_declare_transaction_hash, m)?)?;
    m.add_function(wrap_pyfunction!(py_calculate_deploy_transaction_hash, m)?)?;
    m.add_function(wrap_pyfunction!(
        py_calculate_contract_address_from_hash,
        m
    )?)?;
    m.add_function(wrap_pyfunction!(py_calculate_contract_address, m)?)?;
    m.add_function(wrap_pyfunction!(py_calculate_event_hash, m)?)?;
    m.add_function(wrap_pyfunction!(py_validate_contract_deployed, m)?)?;
    m.add_function(wrap_pyfunction!(py_compute_class_hash, m)?)?;
    m.add_function(wrap_pyfunction!(py_calculate_tx_fee, m)?)?;

    // ~~~~~~~~~~~~~~~~~~~~
    //  Exported Constants
    // ~~~~~~~~~~~~~~~~~~~~

    m.add("DEFAULT_GAS_PRICE", DEFAULT_GAS_PRICE)?;

    // in cairo-lang they're equal
    m.add("DEFAULT_MAX_STEPS", DEFAULT_VALIDATE_MAX_N_STEPS)?;
    m.add("DEFAULT_VALIDATE_MAX_STEPS", DEFAULT_VALIDATE_MAX_N_STEPS)?;

    m.add("DEFAULT_CHAIN_ID", PyStarknetChainId::TestNet)?;
    m.add(
        "DEFAULT_SEQUENCER_ADDRESS",
        DEFAULT_SEQUENCER_ADDRESS.0.to_biguint(),
    )?;

    // The sender address used by default in declare transactions of version 0.
    m.add(
        "DEFAULT_DECLARE_SENDER_ADDRESS",
        Felt252::from(1).to_biguint(),
    )?;

    // OS context offset.
    m.add("SYSCALL_PTR_OFFSET", Felt252::from(0).to_biguint())?;

    // open_zeppelin's account contract
    m.add(
        "account_contract",
        PyContractClass {
            inner: ContractClass::try_from(include_str!("../../../starknet_programs/Account.json"))
                .expect("program couldn't be parsed"),
        },
    )?;

    m.add("LATEST_BLOCK_ID", "latest")?;
    m.add("PENDING_BLOCK_ID", "pending")?;

    m.add(
        "FAULTY_CLASS_HASH",
        felt_str!("748273551117330086452242911863358006088276559700222288674638023319407008063")
            .to_biguint(),
    )?;

    m.add("UNINITIALIZED_CLASS_HASH", *UNINITIALIZED_CLASS_HASH)?;

    // Indentation for transactions meant to query and not addressed to the OS.
    let query_version_base = Felt252::from(1).shl(128u32); // == 2 ** 128
    let query_version = query_version_base + Felt252::from(TRANSACTION_VERSION);
    m.add("QUERY_VERSION", query_version.to_biguint())?;

    m.add("TRANSACTION_VERSION", TRANSACTION_VERSION)?;

    // The (empirical) L1 gas cost of each Cairo step.
    pub const N_STEPS_FEE_WEIGHT: f64 = 0.05;
    m.add("N_STEPS_FEE_WEIGHT", N_STEPS_FEE_WEIGHT)?;

    m.add(
        "CONTRACT_STATES_COMMITMENT_TREE_HEIGHT",
        DEFAULT_CONTRACT_STORAGE_COMMITMENT_TREE_HEIGHT,
    )?;

    m.add("EVENT_COMMITMENT_TREE_HEIGHT", 64)?;
    m.add("TRANSACTION_COMMITMENT_TREE_HEIGHT", 64)?;

    // Felt252 number of bits
    m.add("CONTRACT_ADDRESS_BITS", 251)?;

    // ~~~~~~~~~~~~
    //  Reexported
    // ~~~~~~~~~~~~

    reexport(
        py,
        m,
        "starkware.starknet.definitions.error_codes",
        vec!["StarknetErrorCode"],
    )?;

    reexport(
        py,
        m,
        "starkware.starknet.services.api.gateway.transaction",
        vec![
            "AccountTransaction",
            "Declare",
            "DeployAccount",
            "InvokeFunction",
            "Deploy",
        ],
    )?;

    reexport(
        py,
        m,
        "starkware.starknet.services.api.feeder_gateway.response_objects",
        vec![
            "FeeEstimationInfo",
            "StorageEntry",    // alias Tuple[int, int]
            "BlockIdentifier", // Union[int, Literal["latest"], Literal["pending"]]
            "StateDiff",
            "BlockStateUpdate",
            "BlockStatus",
            "BlockTransactionTraces",
            "TransactionSimulationInfo",
            "StarknetBlock",
            "TransactionInfo",
            "TransactionReceipt",
            "TransactionStatus",
            "TransactionTrace",
            "TransactionExecution",
            "TransactionSpecificInfo",
            "Event",
            "FunctionInvocation",
            "L2ToL1Message",
            "DeclareSpecificInfo",
            "DeployAccountSpecificInfo",
            "DeploySpecificInfo",
            "InvokeSpecificInfo",
            "L1HandlerSpecificInfo",
        ],
    )?;

    reexport(
        py,
        m,
        "starkware.starknet.services.api.feeder_gateway.request_objects",
        vec!["CallL1Handler", "CallFunction"],
    )?;

    reexport(
        py,
        m,
        "starkware.starknet.services.api.feeder_gateway.feeder_gateway_client",
        vec!["FeederGatewayClient"],
    )?;

    reexport(
        py,
        m,
        "starkware.starknet.testing.starknet",
        vec!["Starknet"],
    )?;

    reexport(
        py,
        m,
        "starkware.starknet.business_logic.execution.objects",
        vec!["ResourcesMapping"],
    )?;

    reexport(
        py,
        m,
        "starkware.starknet.business_logic.state.state_api",
        vec!["SyncState", "StateReader"],
    )?;

    // TODO: check
    reexport(
        py,
        m,
        "starkware.starknet.testing.contract",
        vec!["StarknetContract"],
    )?;

    // TODO: check
    reexport(
        py,
        m,
        "starkware.starknet.business_logic.transaction.objects",
        vec!["InternalAccountTransaction", "InternalTransaction"],
    )?;

    reexport(
        py,
        m,
        "starkware.starknet.wallets.open_zeppelin",
        vec!["sign_deploy_account_tx", "sign_invoke_tx"],
    )?;

    reexport(
        py,
        m,
        "starkware.starknet.public.abi",
        vec![
            "AbiEntryType", // alias Dict[str, Any]
            // TODO: export from starknet-rs when implemented
            "get_selector_from_name",
            "get_storage_var_address",
        ],
    )?;

    // TODO: export from starknet-rs when implemented
    reexport(
        py,
        m,
        "starkware.starknet.core.os.block_hash.block_hash",
        vec!["calculate_block_hash"],
    )?;

    reexport(
        py,
        m,
        "starkware.starknet.business_logic.utils",
        vec!["verify_version"],
    )?;

    reexport(
        py,
        m,
        "starkware.starknet.services.api.gateway.transaction_utils",
        vec!["compress_program", "decompress_program"],
    )?;

    reexport(
        py,
        m,
        "starkware.starknet.cli.starknet_cli",
        vec!["get_salt"],
    )?;

    Ok(())
}

fn reexport(py: Python, dst: &PyModule, src_name: &str, names: Vec<&str>) -> PyResult<()> {
    let src = PyModule::import(py, src_name)?;
    for name in names {
        dst.add(name, src.getattr(name)?)?;
    }

    Ok(())
}

#[cfg(test)]
mod test {
    // use pyo3::prelude::*;

    //TODO fix this test in the CI
    // #[test]
    // fn starknet_rs_py_test() {
    //     Python::with_gil(|py| {
    //         // try loading our module
    //         let module = PyModule::new(py, "My Module");
    //         let res = crate::starknet_rs_py(py, module.unwrap());
    //         assert!(res.is_ok(), "{res:?}");
    //     });
    // }
}
