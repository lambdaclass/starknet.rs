#[starknet::interface]
trait IContractTrait<TContractState> {
    fn call_callee_contract(ref self: TContractState, function_selector: felt252) -> felt252;
}

#[starknet::contract]
mod Caller {
    use starknet::call_contract_syscall;
    use core::array;
    use core::result::ResultTrait;

    #[storage]
    struct Storage {
        balance: felt252,
    }

    #[abi(embed_v0)]
    impl IContractTrait of super::IContractTrait<ContractState> {
        fn call_callee_contract(ref self: ContractState, function_selector: felt252) -> felt252 {
            let calldata: Array<felt252> = ArrayTrait::new();
            let callee_addr = starknet::get_contract_address();
            let return_data = call_contract_syscall(callee_addr, function_selector, calldata.span())
                .unwrap();
            *return_data.get(0_usize).unwrap().unbox()
        }
    }
}
