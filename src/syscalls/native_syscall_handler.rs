use cairo_native::starknet::{
    BlockInfo, ExecutionInfo, StarkNetSyscallHandler, SyscallResult, TxInfo, U256,
};
use cairo_vm::felt::Felt252;
use num_traits::Zero;

use crate::{
    core::errors::state_errors::StateError,
    definitions::block_context::BlockContext,
    execution::{
        execution_entry_point::{ExecutionEntryPoint, ExecutionResult},
        CallInfo, CallType, OrderedEvent, OrderedL2ToL1Message, TransactionExecutionContext,
    },
    state::{
        contract_storage_state::ContractStorageState, state_api::StateReader,
        ExecutionResourcesManager,
    },
    utils::Address,
    EntryPointType,
};

#[derive(Debug)]
pub struct NativeSyscallHandler<'a, S>
where
    S: StateReader,
{
    pub(crate) starknet_storage_state: ContractStorageState<'a, S>,
    pub(crate) contract_address: Address,
    pub(crate) caller_address: Address,
    pub(crate) entry_point_selector: Felt252,
    pub(crate) events: Vec<OrderedEvent>,
    pub(crate) n_emitted_events: u64,
    pub(crate) n_sent_messages: usize,
    pub(crate) l2_to_l1_messages: Vec<OrderedL2ToL1Message>,
    pub(crate) tx_execution_context: TransactionExecutionContext,
    pub(crate) block_context: BlockContext,
    // TODO: This may not be really needed for Cairo Native, just passing
    // it to be able to call the `execute` method of ExecutionEntrypoint.
    pub(crate) internal_calls: Vec<CallInfo>,
}

impl<'a, S: StateReader> StarkNetSyscallHandler for NativeSyscallHandler<'a, S> {
    fn get_block_hash(
        &self,
        block_number: u64,
        _gas: &mut u64,
    ) -> SyscallResult<cairo_vm::felt::Felt252> {
        println!("Called `get_block_hash({block_number})` from MLIR.");
        Ok(Felt252::from_bytes_be(b"get_block_hash ok"))
    }

    fn get_execution_info(
        &self,
        _gas: &mut u64,
    ) -> SyscallResult<cairo_native::starknet::ExecutionInfo> {
        println!("Called `get_execution_info()` from MLIR.");
        Ok(ExecutionInfo {
            block_info: BlockInfo {
                block_number: self.block_context.block_info.block_number,
                block_timestamp: self.block_context.block_info.block_timestamp,
                sequencer_address: self.block_context.block_info.sequencer_address.0.clone(),
            },
            tx_info: TxInfo {
                version: self.tx_execution_context.version.clone(),
                account_contract_address: self
                    .tx_execution_context
                    .account_contract_address
                    .0
                    .clone(),
                max_fee: self.tx_execution_context.max_fee,
                signature: self.tx_execution_context.signature.clone(),
                transaction_hash: self.tx_execution_context.transaction_hash.clone(),
                chain_id: self.block_context.starknet_os_config.chain_id.clone(),
                nonce: self.tx_execution_context.nonce.clone(),
            },
            caller_address: self.caller_address.0.clone(),
            contract_address: self.contract_address.0.clone(),
            entry_point_selector: self.entry_point_selector.clone(),
        })
    }

    fn deploy(
        &self,
        class_hash: cairo_vm::felt::Felt252,
        contract_address_salt: cairo_vm::felt::Felt252,
        calldata: &[cairo_vm::felt::Felt252],
        deploy_from_zero: bool,
        _gas: &mut u64,
    ) -> SyscallResult<(cairo_vm::felt::Felt252, Vec<cairo_vm::felt::Felt252>)> {
        println!("Called `deploy({class_hash}, {contract_address_salt}, {calldata:?}, {deploy_from_zero})` from MLIR.");
        Ok((
            class_hash + contract_address_salt,
            calldata.iter().map(|x| x + &Felt252::new(1)).collect(),
        ))
    }

    fn replace_class(
        &self,
        class_hash: cairo_vm::felt::Felt252,
        _gas: &mut u64,
    ) -> SyscallResult<()> {
        println!("Called `replace_class({class_hash})` from MLIR.");
        Ok(())
    }

    fn library_call(
        &self,
        class_hash: cairo_vm::felt::Felt252,
        function_selector: cairo_vm::felt::Felt252,
        calldata: &[cairo_vm::felt::Felt252],
        _gas: &mut u64,
    ) -> SyscallResult<Vec<cairo_vm::felt::Felt252>> {
        println!(
            "Called `library_call({class_hash}, {function_selector}, {calldata:?})` from MLIR."
        );
        Ok(calldata.iter().map(|x| x * &Felt252::new(3)).collect())
    }

    fn call_contract(
        &mut self,
        address: cairo_vm::felt::Felt252,
        entrypoint_selector: cairo_vm::felt::Felt252,
        calldata: &[cairo_vm::felt::Felt252],
        _gas: &mut u64,
    ) -> SyscallResult<Vec<cairo_vm::felt::Felt252>> {
        println!(
            "Called `call_contract({address}, {entrypoint_selector}, {calldata:?})` from MLIR."
        );
        let address = Address(address);
        let exec_entry_point = ExecutionEntryPoint::new(
            address,
            calldata.to_vec(),
            entrypoint_selector,
            self.caller_address.clone(),
            EntryPointType::External,
            Some(CallType::Call),
            None,
            // TODO: The remaining gas must be correctly set someway
            u128::MAX,
        );

        let ExecutionResult { call_info, .. } = exec_entry_point
            .execute(
                self.starknet_storage_state.state,
                // TODO: This fields dont make much sense in the Cairo Native context,
                // they are only dummy values for the `execute` method.
                &BlockContext::default(),
                &mut ExecutionResourcesManager::default(),
                &mut TransactionExecutionContext::default(),
                false,
                u64::MAX,
            )
            .unwrap();

        let call_info = call_info.unwrap();

        // update syscall handler information
        self.starknet_storage_state
            .read_values
            .extend(call_info.storage_read_values.clone());
        self.starknet_storage_state
            .accessed_keys
            .extend(call_info.accessed_storage_keys.clone());
        self.internal_calls.push(call_info.clone());

        Ok(call_info.retdata)
    }

    fn storage_read(
        &mut self,
        address_domain: u32,
        address: cairo_vm::felt::Felt252,
        _gas: &mut u64,
    ) -> SyscallResult<cairo_vm::felt::Felt252> {
        let value = match self.starknet_storage_state.read(&address.to_be_bytes()) {
            Ok(value) => Ok(dbg!(value)),
            Err(_e @ StateError::Io(_)) => todo!(),
            Err(_) => Ok(dbg!(Felt252::zero())),
        };
        println!("Called `storage_read({address_domain}, {address}) = {value:?}` from MLIR.");
        value
    }

    fn storage_write(
        &mut self,
        address_domain: u32,
        address: cairo_vm::felt::Felt252,
        value: cairo_vm::felt::Felt252,
        _gas: &mut u64,
    ) -> SyscallResult<()> {
        println!("Called `storage_write({address_domain}, {address}, {value})` from MLIR.");
        self.starknet_storage_state
            .write(&address.to_be_bytes(), value);
        Ok(())
    }

    fn emit_event(
        &mut self,
        keys: &[cairo_vm::felt::Felt252],
        data: &[cairo_vm::felt::Felt252],
        _gas: &mut u64,
    ) -> SyscallResult<()> {
        let order = self.n_emitted_events;
        println!("Called `emit_event(KEYS: {keys:?}, DATA: {data:?})` from MLIR.");
        self.events
            .push(OrderedEvent::new(order, keys.to_vec(), data.to_vec()));
        self.n_emitted_events += 1;
        Ok(())
    }

    fn send_message_to_l1(
        &mut self,
        to_address: cairo_vm::felt::Felt252,
        payload: &[cairo_vm::felt::Felt252],
        _gas: &mut u64,
    ) -> SyscallResult<()> {
        println!("Called `send_message_to_l1({to_address}, {payload:?})` from MLIR.");
        let addr = Address(to_address);
        self.l2_to_l1_messages.push(OrderedL2ToL1Message::new(
            self.n_sent_messages,
            addr,
            payload.to_vec(),
        ));

        // Update messages count.
        self.n_sent_messages += 1;
        Ok(())
    }

    fn keccak(&self, input: &[u64], _gas: &mut u64) -> SyscallResult<cairo_native::starknet::U256> {
        println!("Called `keccak({input:?})` from MLIR.");
        Ok(U256(Felt252::from(1234567890).to_le_bytes()))
    }

    fn secp256k1_add(
        &self,
        _p0: cairo_native::starknet::Secp256k1Point,
        _p1: cairo_native::starknet::Secp256k1Point,
        _gas: &mut u64,
    ) -> SyscallResult<Option<cairo_native::starknet::Secp256k1Point>> {
        todo!()
    }

    fn secp256k1_get_point_from_x(
        &self,
        _x: cairo_native::starknet::U256,
        _y_parity: bool,
        _gas: &mut u64,
    ) -> SyscallResult<Option<cairo_native::starknet::Secp256k1Point>> {
        todo!()
    }

    fn secp256k1_get_xy(
        &self,
        _p: cairo_native::starknet::Secp256k1Point,
        _gas: &mut u64,
    ) -> SyscallResult<(cairo_native::starknet::U256, cairo_native::starknet::U256)> {
        todo!()
    }

    fn secp256k1_mul(
        &self,
        _p: cairo_native::starknet::Secp256k1Point,
        _m: cairo_native::starknet::U256,
        _gas: &mut u64,
    ) -> SyscallResult<Option<cairo_native::starknet::Secp256k1Point>> {
        todo!()
    }

    fn secp256k1_new(
        &self,
        _x: cairo_native::starknet::U256,
        _y: cairo_native::starknet::U256,
        _gas: &mut u64,
    ) -> SyscallResult<Option<cairo_native::starknet::Secp256k1Point>> {
        todo!()
    }

    fn secp256r1_add(
        &self,
        _p0: cairo_native::starknet::Secp256k1Point,
        _p1: cairo_native::starknet::Secp256k1Point,
        _gas: &mut u64,
    ) -> SyscallResult<Option<cairo_native::starknet::Secp256k1Point>> {
        todo!()
    }

    fn secp256r1_get_point_from_x(
        &self,
        _x: cairo_native::starknet::U256,
        _y_parity: bool,
        _gas: &mut u64,
    ) -> SyscallResult<Option<cairo_native::starknet::Secp256k1Point>> {
        todo!()
    }

    fn secp256r1_get_xy(
        &self,
        _p: cairo_native::starknet::Secp256k1Point,
        _gas: &mut u64,
    ) -> SyscallResult<(cairo_native::starknet::U256, cairo_native::starknet::U256)> {
        todo!()
    }

    fn secp256r1_mul(
        &self,
        _p: cairo_native::starknet::Secp256k1Point,
        _m: cairo_native::starknet::U256,
        _gas: &mut u64,
    ) -> SyscallResult<Option<cairo_native::starknet::Secp256k1Point>> {
        todo!()
    }

    fn secp256r1_new(
        &self,
        _x: cairo_native::starknet::U256,
        _y: cairo_native::starknet::U256,
        _gas: &mut u64,
    ) -> SyscallResult<Option<cairo_native::starknet::Secp256k1Point>> {
        todo!()
    }

    fn pop_log(&self) {
        todo!()
    }

    fn set_account_contract_address(&self, _contract_address: cairo_vm::felt::Felt252) {
        todo!()
    }

    fn set_block_number(&self, _block_number: u64) {
        todo!()
    }

    fn set_block_timestamp(&self, _block_timestamp: u64) {
        todo!()
    }

    fn set_caller_address(&self, _address: cairo_vm::felt::Felt252) {
        todo!()
    }

    fn set_chain_id(&self, _chain_id: cairo_vm::felt::Felt252) {
        todo!()
    }

    fn set_contract_address(&self, _address: cairo_vm::felt::Felt252) {
        todo!()
    }

    fn set_max_fee(&self, _max_fee: u128) {
        todo!()
    }

    fn set_nonce(&self, _nonce: cairo_vm::felt::Felt252) {
        todo!()
    }

    fn set_sequencer_address(&self, _address: cairo_vm::felt::Felt252) {
        todo!()
    }

    fn set_signature(&self, _signature: &[cairo_vm::felt::Felt252]) {
        todo!()
    }

    fn set_transaction_hash(&self, _transaction_hash: cairo_vm::felt::Felt252) {
        todo!()
    }

    fn set_version(&self, _version: cairo_vm::felt::Felt252) {
        todo!()
    }
}
