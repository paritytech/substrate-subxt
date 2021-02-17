// Copyright 2019-2021 Parity Technologies (UK) Ltd.
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

use codec::{
    Decode,
    Encode,
};
use core::{
    fmt::Debug,
    marker::PhantomData,
};
use sp_runtime::{
    generic::Era,
    traits::SignedExtension,
    transaction_validity::TransactionValidityError,
};

use crate::{
    frame::{
        balances::Balances,
        system::System,
    },
    runtimes::Runtime,
};

/// Extra type.
pub type Extra<T> = <<T as Runtime>::Extra as SignedExtra<T>>::Extra;

/// SignedExtra checks copied from substrate, in order to remove requirement to implement
/// substrate's `frame_system::Trait`

/// Ensure the runtime version registered in the transaction is the same as at present.
///
/// # Note
///
/// This is modified from the substrate version to allow passing in of the version, which is
/// returned via `additional_signed()`.

/// Ensure the runtime version registered in the transaction is the same as at present.
#[derive(Encode, Decode, Clone, Eq, PartialEq, Debug)]
pub struct CheckSpecVersion<T: System>(
    pub PhantomData<T>,
    /// Local version to be used for `AdditionalSigned`
    #[codec(skip)]
    pub u32,
);

impl<T> SignedExtension for CheckSpecVersion<T>
where
    T: System + Clone + Debug + Eq + Send + Sync,
{
    const IDENTIFIER: &'static str = "CheckSpecVersion";
    type AccountId = u64;
    type Call = ();
    type AdditionalSigned = u32;
    type Pre = ();
    fn additional_signed(
        &self,
    ) -> Result<Self::AdditionalSigned, TransactionValidityError> {
        Ok(self.1)
    }
}

/// Ensure the transaction version registered in the transaction is the same as at present.
///
/// # Note
///
/// This is modified from the substrate version to allow passing in of the version, which is
/// returned via `additional_signed()`.
#[derive(Encode, Decode, Clone, Eq, PartialEq, Debug)]
pub struct CheckTxVersion<T: System>(
    pub PhantomData<T>,
    /// Local version to be used for `AdditionalSigned`
    #[codec(skip)]
    pub u32,
);

impl<T> SignedExtension for CheckTxVersion<T>
where
    T: System + Clone + Debug + Eq + Send + Sync,
{
    const IDENTIFIER: &'static str = "CheckTxVersion";
    type AccountId = u64;
    type Call = ();
    type AdditionalSigned = u32;
    type Pre = ();
    fn additional_signed(
        &self,
    ) -> Result<Self::AdditionalSigned, TransactionValidityError> {
        Ok(self.1)
    }
}

/// Check genesis hash
///
/// # Note
///
/// This is modified from the substrate version to allow passing in of the genesis hash, which is
/// returned via `additional_signed()`.
#[derive(Encode, Decode, Clone, Eq, PartialEq, Debug)]
pub struct CheckGenesis<T: System>(
    pub PhantomData<T>,
    /// Local genesis hash to be used for `AdditionalSigned`
    #[codec(skip)]
    pub T::Hash,
);

impl<T> SignedExtension for CheckGenesis<T>
where
    T: System + Clone + Debug + Eq + Send + Sync,
{
    const IDENTIFIER: &'static str = "CheckGenesis";
    type AccountId = u64;
    type Call = ();
    type AdditionalSigned = T::Hash;
    type Pre = ();
    fn additional_signed(
        &self,
    ) -> Result<Self::AdditionalSigned, TransactionValidityError> {
        Ok(self.1)
    }
}

/// Check for transaction mortality.
///
/// # Note
///
/// This is modified from the substrate version to allow passing in a hash (either the genesis hash
/// if immortal or current hash if mortal. The hash is returned via `additional_signed()`.
#[derive(Encode, Decode, Clone, Eq, PartialEq, Debug)]
pub struct CheckEra<T: System>(
    /// The default structure for the Extra encoding
    pub (Era, PhantomData<T>),
    /// Local hash to be used for `AdditionalSigned`
    #[codec(skip)]
    pub T::Hash,
);

impl<T> SignedExtension for CheckEra<T>
where
    T: System + Clone + Debug + Eq + Send + Sync,
{
    const IDENTIFIER: &'static str = "CheckEra";
    type AccountId = u64;
    type Call = ();
    type AdditionalSigned = T::Hash;
    type Pre = ();
    fn additional_signed(
        &self,
    ) -> Result<Self::AdditionalSigned, TransactionValidityError> {
        Ok(self.1)
    }
}

/// Nonce check and increment to give replay protection for transactions.
#[derive(Encode, Decode, Clone, Eq, PartialEq, Debug)]
pub struct CheckNonce<T: System>(#[codec(compact)] pub T::Index);

impl<T> SignedExtension for CheckNonce<T>
where
    T: System + Clone + Debug + Eq + Send + Sync,
{
    const IDENTIFIER: &'static str = "CheckNonce";
    type AccountId = u64;
    type Call = ();
    type AdditionalSigned = ();
    type Pre = ();
    fn additional_signed(
        &self,
    ) -> Result<Self::AdditionalSigned, TransactionValidityError> {
        Ok(())
    }
}

/// Resource limit check.
#[derive(Encode, Decode, Clone, Eq, PartialEq, Debug)]
pub struct CheckWeight<T: System>(pub PhantomData<T>);

impl<T> SignedExtension for CheckWeight<T>
where
    T: System + Clone + Debug + Eq + Send + Sync,
{
    const IDENTIFIER: &'static str = "CheckWeight";
    type AccountId = u64;
    type Call = ();
    type AdditionalSigned = ();
    type Pre = ();
    fn additional_signed(
        &self,
    ) -> Result<Self::AdditionalSigned, TransactionValidityError> {
        Ok(())
    }
}

/// Require the transactor pay for themselves and maybe include a tip to gain additional priority
/// in the queue.
#[derive(Encode, Decode, Clone, Eq, PartialEq, Debug)]
pub struct ChargeTransactionPayment<T: Balances>(#[codec(compact)] pub T::Balance);

impl<T> SignedExtension for ChargeTransactionPayment<T>
where
    T: Balances + Clone + Debug + Eq + Send + Sync,
{
    const IDENTIFIER: &'static str = "ChargeTransactionPayment";
    type AccountId = u64;
    type Call = ();
    type AdditionalSigned = ();
    type Pre = ();
    fn additional_signed(
        &self,
    ) -> Result<Self::AdditionalSigned, TransactionValidityError> {
        Ok(())
    }
}

/// Trait for implementing transaction extras for a runtime.
pub trait SignedExtra<T: System>: SignedExtension {
    /// The type the extras.
    type Extra: SignedExtension + Send + Sync;

    /// Creates a new `SignedExtra`.
    fn new(
        spec_version: u32,
        tx_version: u32,
        nonce: T::Index,
        genesis_hash: T::Hash,
        era_info: (Era, Option<T::Hash>),
    ) -> Self;

    /// Returns the transaction extra.
    fn extra(&self) -> Self::Extra;
}

/// Default `SignedExtra` for substrate runtimes.
#[derive(Encode, Decode, Clone, Eq, PartialEq, Debug)]
pub struct DefaultExtra<T: System> {
    spec_version: u32,
    tx_version: u32,
    nonce: T::Index,
    genesis_hash: T::Hash,
    era_info: (Era, Option<T::Hash>),
}

impl<T: System + Balances + Clone + Debug + Eq + Send + Sync> SignedExtra<T>
    for DefaultExtra<T>
{
    type Extra = (
        CheckSpecVersion<T>,
        CheckTxVersion<T>,
        CheckGenesis<T>,
        CheckEra<T>,
        CheckNonce<T>,
        CheckWeight<T>,
        ChargeTransactionPayment<T>,
    );

    fn new(
        spec_version: u32,
        tx_version: u32,
        nonce: T::Index,
        genesis_hash: T::Hash,
        era_info: (Era, Option<T::Hash>),
    ) -> Self {
        DefaultExtra {
            spec_version,
            tx_version,
            nonce,
            genesis_hash,
            era_info,
        }
    }

    fn extra(&self) -> Self::Extra {
        let era_hash = if let Some(hash) = self.era_info.1 {
            hash
        } else {
            self.genesis_hash
        };
        (
            CheckSpecVersion(PhantomData, self.spec_version),
            CheckTxVersion(PhantomData, self.tx_version),
            CheckGenesis(PhantomData, self.genesis_hash),
            CheckEra((self.era_info.0, PhantomData), era_hash),
            CheckNonce(self.nonce),
            CheckWeight(PhantomData),
            ChargeTransactionPayment(<T as Balances>::Balance::default()),
        )
    }
}

impl<T: System + Balances + Clone + Debug + Eq + Send + Sync> SignedExtension
    for DefaultExtra<T>
{
    const IDENTIFIER: &'static str = "DefaultExtra";
    type AccountId = T::AccountId;
    type Call = ();
    type AdditionalSigned =
        <<Self as SignedExtra<T>>::Extra as SignedExtension>::AdditionalSigned;
    type Pre = ();

    fn additional_signed(
        &self,
    ) -> Result<Self::AdditionalSigned, TransactionValidityError> {
        self.extra().additional_signed()
    }
}
