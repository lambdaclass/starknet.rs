use starknet::{secp256k1, secp256r1};

#[starknet::interface]
trait ISecp256k1MulTest<TContractState> {
   fn secp256k1_mul_test(self: @TContractState, n: u256) -> (u256, u256);
}

#[starknet::contract]
mod Secp256k1MulTest {
    use core::starknet::secp256_trait::Secp256PointTrait;
use core::result::ResultTrait;
use starknet::{secp256k1, secp256r1};
    use starknet::secp256k1::Secp256k1Impl;

    #[storage]
    struct Storage {}

    #[abi(embed_v0)]
    impl Secp256k1MulTest of super::ISecp256k1MulTest<ContractState> {
        
        fn secp256k1_mul_test(self: @ContractState, n: u256) -> (u256, u256) {
            let point: secp256k1::Secp256k1Point = Secp256k1Impl::get_generator_point();
            let scalar: u256 = u256 { high: 0, low: 1 };
            let result = secp256k1::secp256k1_mul_syscall(point, scalar);
            result.unwrap().get_coordinates().unwrap()
        }
    }
}
