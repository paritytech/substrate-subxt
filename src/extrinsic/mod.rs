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

mod extra;
mod signer;

pub use self::{
    extra::{
        DefaultExtra,
        SignedExtension,
        SignedExtra,
    },
    signer::{
        Signer,
        PairSigner,
    }
};

use codec::Decode;
use futures::future;
use jsonrpsee::client::Subscription;
use sc_rpc_api::state::ReadProof;
use sp_core::storage::{
    StorageChangeSet,
    StorageKey,
};
use sp_runtime::traits::{
    SignedExtension,
};
use sp_version::RuntimeVersion;

use crate::{
    Encoded,
    Error,
    frame::{
        system::{
            System,
        }
    },
    rpc::{
        ChainBlock,
        Rpc,
    },
    runtimes::Runtime,
};

/// UncheckedExtrinsic type.
pub type UncheckedExtrinsic<T> = sp_runtime::generic::UncheckedExtrinsic<
    <T as System>::Address,
    Encoded,
    <T as Runtime>::Signature,
    Extra<T>,
>;

/// SignedPayload type.
pub type SignedPayload<T> = sp_runtime::generic::SignedPayload<Encoded, Extra<T>>;

/// Creates a signed extrinsic
pub fn create_signed<T, S>(
    runtime_version: &RuntimeVersion,
    genesis_hash: T::Hash,
    nonce: T::Index,
    call: Encoded,
    signer: &(dyn Signer<T> + Send + Sync),
) -> Result<UncheckedExtrinsic<T>, Error>
where
    T: Runtime,
    <<T::Extra as SignedExtra<T>>::Extra as SignedExtension>::AdditionalSigned: Send + Sync,
{
    let spec_version = runtime_version.spec_version;
    let tx_version = runtime_version.transaction_version;
    let extra: T::Extra =
        T::Extra::new(spec_version, tx_version, nonce, genesis_hash);
    let payload = SignedPayload::<T>::new(call, extra.extra())?;
    let signed = signer.sign(payload).await?;
    Ok(signed)
}

/// Creates an unsigned extrinsic
pub fn create_unsigned<T>(call: Encoded) -> UncheckedExtrinsic<T>
where
    T: Runtime,
{
    UncheckedExtrinsic::<T>::new_unsigned(call)
}
