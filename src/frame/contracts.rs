// Copyright 2019-2020 Parity Technologies (UK) Ltd.
// This file is part of substrate-subxt.
//
// subxt is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// subxt is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with substrate-subxt.  If not, see <http://www.gnu.org/licenses/>.

//! Implements support for the pallet_contracts module.

use crate::frame::{
    balances::{
        Balances,
        BalancesEventsDecoder,
    },
    system::{
        System,
        SystemEventsDecoder,
    },
};
use codec::{
    Decode,
    Encode,
};
use core::marker::PhantomData;

/// Gas units are chosen to be represented by u64 so that gas metering
/// instructions can operate on them efficiently.
pub type Gas = u64;

/// The subset of the `pallet_contracts::Trait` that a client must implement.
#[module]
pub trait Contracts: System + Balances {}

/// Stores the given binary Wasm code into the chain's storage and returns
/// its `codehash`.
/// You can instantiate contracts only with stored code.
#[derive(Clone, Debug, Eq, PartialEq, Call, Encode)]
pub struct PutCodeCall<'a, T: Contracts> {
    /// Runtime marker.
    pub _runtime: PhantomData<T>,
    /// Wasm blob.
    pub code: &'a [u8],
}

/// Creates a new contract from the `codehash` generated by `put_code`,
/// optionally transferring some balance.
///
/// Creation is executed as follows:
///
/// - The destination address is computed based on the sender and hash of
/// the code.
/// - The smart-contract account is instantiated at the computed address.
/// - The `ctor_code` is executed in the context of the newly-instantiated
/// account. Buffer returned after the execution is saved as the `code`https://www.bbc.co.uk/
/// of the account. That code will be invoked upon any call received by
/// this account.
/// - The contract is initialized.
#[derive(Clone, Debug, Eq, PartialEq, Call, Encode)]
pub struct InstantiateCall<'a, T: Contracts> {
    /// Initial balance transfered to the contract.
    #[codec(compact)]
    pub endowment: <T as Balances>::Balance,
    /// Gas limit.
    #[codec(compact)]
    pub gas_limit: Gas,
    /// Code hash returned by the put_code call.
    pub code_hash: &'a <T as System>::Hash,
    /// Data to initialize the contract with.
    pub data: &'a [u8],
}

/// Makes a call to an account, optionally transferring some balance.
///
/// * If the account is a smart-contract account, the associated code will
///  be executed and any value will be transferred.
/// * If the account is a regular account, any value will be transferred.
/// * If no account exists and the call value is not less than
/// `existential_deposit`, a regular account will be created and any value
///  will be transferred.
#[derive(Clone, Debug, PartialEq, Call, Encode)]
pub struct CallCall<'a, T: Contracts> {
    /// Address of the contract.
    pub dest: &'a <T as System>::Address,
    /// Value to transfer to the contract.
    pub value: <T as Balances>::Balance,
    /// Gas limit.
    #[codec(compact)]
    pub gas_limit: Gas,
    /// Data to send to the contract.
    pub data: &'a [u8],
}

/// Code stored event.
#[derive(Clone, Debug, Eq, PartialEq, Event, Decode)]
pub struct CodeStoredEvent<T: Contracts> {
    /// Code hash of the contract.
    pub code_hash: T::Hash,
}

/// Instantiated event.
#[derive(Clone, Debug, Eq, PartialEq, Event, Decode)]
pub struct InstantiatedEvent<T: Contracts> {
    /// Caller that instantiated the contract.
    pub caller: <T as System>::AccountId,
    /// The address of the contract.
    pub contract: <T as System>::AccountId,
}

#[cfg(test)]
mod tests {
    use sp_keyring::AccountKeyring;

    use super::*;
    use crate::{
        balances::*,
        Client,
        ClientBuilder,
        ContractsTemplateRuntime,
        Error,
        PairSigner,
    };
    use sp_core::sr25519::Pair;
    use sp_core::crypto::AccountId32;

    async fn new_client() -> Client<ContractsTemplateRuntime> {
        ClientBuilder::<ContractsTemplateRuntime>::new()
            .build()
            .await
            .expect("Error creating client")
    }

    async fn put_code(signer: &PairSigner<ContractsTemplateRuntime, Pair>) -> Result<CodeStoredEvent<ContractsTemplateRuntime>, Error> {
        const CONTRACT: &str = r#"
            (module
                (func (export "call"))
                (func (export "deploy"))
            )
        "#;
        let code = wabt::wat2wasm(CONTRACT).expect("invalid wabt");

        let client = new_client().await;

        let result = client.put_code_and_watch(signer, &code).await?;
        result
            .code_stored()?
            .ok_or_else(|| "Failed to find a CodeStored event".into())
    }

    /// generate a new keypair for an account, and fund it so it can perform smart contract operations
    async fn generate_account() -> PairSigner<ContractsTemplateRuntime, Pair> {
        use sp_core::Pair as _;
        let new_account = Pair::generate().0;
        let new_account_id: AccountId32 = new_account.public().into();
        // fund the account
        let benefactor = PairSigner::new(AccountKeyring::Alice.pair());
        let client = new_client().await;
        let endowment = 200_000_000_000_000;
        let _ = client.transfer_and_watch(&benefactor, &new_account_id, endowment)
            .await
            .expect("New account balance transfer failed");
        PairSigner::new(new_account)
    }

    #[async_std::test]
    #[cfg(feature = "integration-tests")]
    async fn tx_put_code() {
        env_logger::try_init().ok();

        let signer = generate_account().await;

        let code_stored = put_code(&signer).await;

        assert!(
            code_stored.is_ok(),
            format!(
                "Error calling put_code and receiving CodeStored Event: {:?}",
                code_stored
            )
        );
    }

    #[async_std::test]
    #[cfg(feature = "integration-tests")]
    async fn tx_instantiate() {
        env_logger::try_init().ok();
        let signer = generate_account().await;

        // call put_code extrinsic
        let code_stored = put_code(&signer).await.unwrap();

        log::info!("Code hash: {:?}", code_stored.code_hash);

        let client = new_client().await;

        // call instantiate extrinsic
        let result = client
            .instantiate_and_watch(
                &signer,
                100_000_000_000_000, // endowment
                500_000_000,         // gas_limit
                &code_stored.code_hash,
                &[], // data
            )
            .await
            .unwrap();

        log::info!("Instantiate result: {:?}", result);
        let event = result.instantiated().unwrap();

        assert!(
            event.is_some(),
            format!("Error instantiating contract: {:?}", result)
        );
    }

    // #[async_std::test]
    // #[cfg(feature = "integration-tests")]
    // async fn tx_call() {
    //
    // }
}
