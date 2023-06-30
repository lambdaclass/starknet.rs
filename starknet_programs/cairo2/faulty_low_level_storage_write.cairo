#[starknet::interface]
trait IWriteStorage<TContractState> {
   fn write_storage(self: @TContractState) -> felt252;
}

#[starknet::contract]
mod WriteStorage {
    use array::{Array, ArrayTrait, Span, SpanTrait};
    use starknet::{syscalls::storage_write_syscall, storage_access::{StorageAddress, storage_address_try_from_felt252}};
    use result::ResultTrait;
    use option::OptionTrait;

    #[storage]
    struct Storage {
    }

    #[external(v0)]
    impl WriteStorage of super::IWriteStorage<ContractState> {
        fn write_storage(self: @ContractState) -> felt252 {
            let address = storage_address_try_from_felt252(1).unwrap();
            match storage_write_syscall(3, address, 1) {
                Result::Ok(value) => 1,
                Result::Err(revert_reason) => *revert_reason.span().at(0),
            }
        }
    }
}
