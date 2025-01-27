use std::collections::HashMap;

use cairo_vm::{types::builtin_name::BuiltinName, vm::runners::cairo_runner::ExecutionResources};

use crate::{definitions::transaction_type::TransactionType, transaction::error::TransactionError};

// Values compatible with blockifier version 0.5.0-dev.0
pub(crate) const ESTIMATED_INVOKE_FUNCTION_STEPS: usize = 3549;
pub(crate) const ESTIMATED_DECLARE_STEPS: usize = 2843;
pub(crate) const ESTIMATED_DEPLOY_STEPS: usize = 0;
pub(crate) const ESTIMATED_DEPLOY_ACCOUNT_STEPS: usize = 3798;
pub(crate) const ESTIMATED_L1_HANDLER_STEPS: usize = 1157;

/// Represents the operating system resources associated with syscalls and transactions.
#[derive(Debug, Clone)]
pub struct OsResources {
    execute_syscalls: HashMap<String, ExecutionResources>,
    execute_txs_inner: HashMap<TransactionType, ExecutionResources>,
}

// Values compatible with blockifier version 0.5.0-dev.0
impl Default for OsResources {
    /// Provide default values for `OsResources`.
    fn default() -> Self {
        let execute_txs_inner: HashMap<TransactionType, ExecutionResources> = HashMap::from([
            (
                TransactionType::InvokeFunction,
                ExecutionResources {
                    n_steps: ESTIMATED_INVOKE_FUNCTION_STEPS,
                    n_memory_holes: 68,
                    builtin_instance_counter: HashMap::from([
                        (BuiltinName::pedersen, 16),
                        (BuiltinName::range_check, 80),
                    ]),
                },
            ),
            (
                TransactionType::Declare,
                ExecutionResources {
                    n_steps: ESTIMATED_DECLARE_STEPS,
                    n_memory_holes: 66,
                    builtin_instance_counter: HashMap::from([
                        (BuiltinName::pedersen, 15),
                        (BuiltinName::range_check, 63),
                    ]),
                },
            ),
            (
                TransactionType::Deploy,
                ExecutionResources {
                    n_steps: ESTIMATED_DEPLOY_STEPS,
                    n_memory_holes: 0,
                    builtin_instance_counter: HashMap::new(),
                },
            ),
            (
                TransactionType::DeployAccount,
                ExecutionResources {
                    n_steps: ESTIMATED_DEPLOY_ACCOUNT_STEPS,
                    n_memory_holes: 82,
                    builtin_instance_counter: HashMap::from([
                        (BuiltinName::pedersen, 23),
                        (BuiltinName::range_check, 83),
                    ]),
                },
            ),
            (
                TransactionType::L1Handler,
                ExecutionResources {
                    n_steps: ESTIMATED_L1_HANDLER_STEPS,
                    n_memory_holes: 0,
                    builtin_instance_counter: HashMap::from([
                        (BuiltinName::pedersen, 11),
                        (BuiltinName::range_check, 17),
                    ]),
                },
            ),
        ]);

        let execute_syscalls = HashMap::from([
            (
                "call_contract".to_string(),
                ExecutionResources {
                    n_steps: 760,
                    n_memory_holes: 4,
                    builtin_instance_counter: HashMap::from([(BuiltinName::range_check, 20)]),
                },
            ),
            (
                "delegate_call".to_string(),
                ExecutionResources {
                    n_steps: 713,
                    n_memory_holes: 0,
                    builtin_instance_counter: HashMap::from([(BuiltinName::range_check, 19)]),
                },
            ),
            (
                "delegate_l1_handler".to_string(),
                ExecutionResources {
                    n_steps: 692,
                    n_memory_holes: 0,
                    builtin_instance_counter: HashMap::from([(BuiltinName::range_check, 15)]),
                },
            ),
            (
                "deploy".to_string(),
                ExecutionResources {
                    n_steps: 1010,
                    n_memory_holes: 13,
                    builtin_instance_counter: HashMap::from([
                        (BuiltinName::range_check, 19),
                        (BuiltinName::pedersen, 7),
                    ]),
                },
            ),
            (
                "emit_event".to_string(),
                ExecutionResources {
                    n_steps: 61,
                    n_memory_holes: 0,
                    builtin_instance_counter: HashMap::from([(BuiltinName::range_check, 1)]),
                },
            ),
            (
                "get_block_hash".to_string(),
                ExecutionResources {
                    n_steps: 74,
                    n_memory_holes: 0,
                    builtin_instance_counter: HashMap::from([(BuiltinName::range_check, 2)]),
                },
            ),
            (
                "get_block_number".to_string(),
                ExecutionResources {
                    n_steps: 40,
                    n_memory_holes: 0,
                    builtin_instance_counter: HashMap::new(),
                },
            ),
            (
                "get_block_timestamp".to_string(),
                ExecutionResources {
                    n_steps: 38,
                    n_memory_holes: 0,
                    builtin_instance_counter: HashMap::new(),
                },
            ),
            (
                "get_caller_address".to_string(),
                ExecutionResources {
                    n_steps: 62,
                    n_memory_holes: 0,
                    builtin_instance_counter: HashMap::from([(BuiltinName::range_check, 1)]),
                },
            ),
            (
                "get_contract_address".to_string(),
                ExecutionResources {
                    n_steps: 62,
                    n_memory_holes: 0,
                    builtin_instance_counter: HashMap::from([(BuiltinName::range_check, 1)]),
                },
            ),
            (
                "get_execution_info".to_string(),
                ExecutionResources {
                    n_steps: 62,
                    n_memory_holes: 0,
                    builtin_instance_counter: HashMap::from([(BuiltinName::range_check, 1)]),
                },
            ),
            (
                "get_sequencer_address".to_string(),
                ExecutionResources {
                    n_steps: 34,
                    n_memory_holes: 0,
                    builtin_instance_counter: HashMap::new(),
                },
            ),
            (
                "get_tx_info".to_string(),
                ExecutionResources {
                    n_steps: 62,
                    n_memory_holes: 0,
                    builtin_instance_counter: HashMap::from([(BuiltinName::range_check, 1)]),
                },
            ),
            (
                "get_tx_signature".to_string(),
                ExecutionResources {
                    n_steps: 44,
                    n_memory_holes: 0,
                    builtin_instance_counter: HashMap::new(),
                },
            ),
            (
                "library_call".to_string(),
                ExecutionResources {
                    n_steps: 751,
                    n_memory_holes: 4,
                    builtin_instance_counter: HashMap::from([(BuiltinName::range_check, 20)]),
                },
            ),
            (
                "library_call_l1_handler".to_string(),
                ExecutionResources {
                    n_steps: 659,
                    n_memory_holes: 0,
                    builtin_instance_counter: HashMap::from([(BuiltinName::range_check, 15)]),
                },
            ),
            (
                "replace_class".to_string(),
                ExecutionResources {
                    n_steps: 98,
                    n_memory_holes: 0,
                    builtin_instance_counter: HashMap::from([(BuiltinName::range_check, 1)]),
                },
            ),
            (
                "send_message_to_l1".to_string(),
                ExecutionResources {
                    n_steps: 139,
                    n_memory_holes: 0,
                    builtin_instance_counter: HashMap::from([(BuiltinName::range_check, 1)]),
                },
            ),
            (
                "storage_read".to_string(),
                ExecutionResources {
                    n_steps: 87,
                    n_memory_holes: 0,
                    builtin_instance_counter: HashMap::from([(BuiltinName::range_check, 1)]),
                },
            ),
            (
                "storage_write".to_string(),
                ExecutionResources {
                    n_steps: 89,
                    n_memory_holes: 0,
                    builtin_instance_counter: HashMap::from([(BuiltinName::range_check, 1)]),
                },
            ),
        ]);

        OsResources {
            execute_syscalls,
            execute_txs_inner,
        }
    }
}

/// Calculate the additional operating system resources required to execute a transaction
/// given a set of syscalls invoked and a transaction type.
pub fn get_additional_os_resources(
    syscall_counter: HashMap<String, u64>,
    tx_type: &TransactionType,
) -> Result<ExecutionResources, TransactionError> {
    let os_resources = OsResources::default();

    let mut additional_os_resources = ExecutionResources::default();

    for (syscall, count) in syscall_counter {
        let syscall_resources = &os_resources
            .execute_syscalls
            .get(&syscall)
            .ok_or_else(|| TransactionError::ResourcesError)?
            .clone()
            * count as usize;

        additional_os_resources += &syscall_resources;
    }

    additional_os_resources += &os_resources
        .execute_txs_inner
        .get(tx_type)
        .ok_or_else(|| TransactionError::NoneTransactionType(*tx_type, os_resources.clone()))?
        .clone();

    Ok(additional_os_resources)
}

/// Test for the `get_additional_os_resources` function.
#[test]
fn get_additional_os_resources_test() {
    let syscall_counter = HashMap::from([("storage_read".into(), 2), ("storage_write".into(), 3)]);

    let tx_type = TransactionType::InvokeFunction;

    let additional_os_resources = get_additional_os_resources(syscall_counter, &tx_type).unwrap();
    let expected_additional_os_resources = ExecutionResources {
        n_steps: 3990,
        n_memory_holes: 68,
        builtin_instance_counter: HashMap::from([
            (BuiltinName::range_check, 85),
            (BuiltinName::pedersen, 16),
        ]),
    };

    assert_eq!(additional_os_resources, expected_additional_os_resources);
}
