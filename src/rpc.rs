// Copyright 2018-2019 Parity Technologies (UK) Ltd.
// This file is part of substrate-subxt.
//
// ink! is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// ink! is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with substrate-subxt.  If not, see <http://www.gnu.org/licenses/>.

use crate::{error::Error, ExtrinsicSuccess};
use futures::{
    future::{self, Future, IntoFuture},
    stream::Stream,
};
use jsonrpc_core_client::{RpcChannel, RpcError, TypedSubscriptionStream};
use log;
use node_runtime::{Call, UncheckedExtrinsic};
use num_traits::bounds::Bounded;
use parity_codec::{Decode, Encode};

use runtime_primitives::{generic::Era, traits::Hash as _};
use runtime_support::StorageMap;
use serde::{self, de::Error as DeError, Deserialize};
use substrate_primitives::{
    blake2_256,
    storage::{StorageChangeSet, StorageKey},
    twox_128, Pair,
};
use substrate_rpc::{
    author::AuthorClient,
    chain::{number::NumberOrHex, ChainClient},
    state::StateClient,
};
use transaction_pool::txpool::watcher::Status;

/// Copy of runtime_primitives::OpaqueExtrinsic to allow a local Deserialize impl
#[derive(PartialEq, Eq, Clone, Default, Encode, Decode)]
pub struct OpaqueExtrinsic(pub Vec<u8>);

impl std::fmt::Debug for OpaqueExtrinsic {
    fn fmt(&self, fmt: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(
            fmt,
            "{}",
            substrate_primitives::hexdisplay::HexDisplay::from(&self.0)
        )
    }
}

impl<'a> serde::Deserialize<'a> for OpaqueExtrinsic {
    fn deserialize<D>(de: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'a>,
    {
        let r = substrate_primitives::bytes::deserialize(de)?;
        Decode::decode(&mut &r[..]).ok_or(DeError::custom("Invalid value passed into decode"))
    }
}

/// Copy of runtime_primitives::generic::Block with Deserialize implemented
#[derive(PartialEq, Eq, Clone, Encode, Decode, Debug, Deserialize)]
pub struct Block {
    // not included: pub header: Header,
    /// The accompanying extrinsics.
    pub extrinsics: Vec<OpaqueExtrinsic>,
}

/// Copy of runtime_primitives::generic::SignedBlock with Deserialize implemented
#[derive(PartialEq, Eq, Clone, Encode, Decode, Debug, Deserialize)]
pub struct SignedBlock {
    /// Full block.
    pub block: Block,
}

/// Client for substrate rpc interfaces
pub struct Rpc<T: srml_system::Trait> {
    state: StateClient<<T as srml_system::Trait>::Hash>,
    chain: ChainClient<
        <T as srml_system::Trait>::BlockNumber,
        <T as srml_system::Trait>::Hash,
        (),
        SignedBlock,
    >,
    author: AuthorClient<<T as srml_system::Trait>::Hash, <T as srml_system::Trait>::Hash>,
}

/// Allows connecting to all inner interfaces on the same RpcChannel
impl<T: srml_system::Trait> From<RpcChannel> for Rpc<T> {
    fn from(channel: RpcChannel) -> Rpc<T> {
        Rpc {
            state: channel.clone().into(),
            chain: channel.clone().into(),
            author: channel.into(),
        }
    }
}

impl<T: srml_system::Trait> Rpc<T> {
    /// Fetch the latest nonce for the given `AccountId`
    fn fetch_nonce(
        &self,
        account: &<T as srml_system::Trait>::AccountId,
    ) -> impl Future<Item = <T as srml_system::Trait>::Index, Error = RpcError> {
        let account_nonce_key = <srml_system::AccountNonce<T>>::key_for(account);
        let storage_key = blake2_256(&account_nonce_key).to_vec();

        self.state
            .storage(StorageKey(storage_key), None)
            .map(|data| {
                data.map_or(Default::default(), |d| {
                    Decode::decode(&mut &d.0[..]).expect("Account nonce is valid Index")
                })
            })
            .map_err(Into::into)
    }

    /// Fetch the genesis hash
    fn fetch_genesis_hash(
        &self,
    ) -> impl Future<Item = Option<<T as srml_system::Trait>::Hash>, Error = RpcError> {
        let block_zero = <T as srml_system::Trait>::BlockNumber::min_value();
        self.chain.block_hash(Some(NumberOrHex::Number(block_zero)))
    }

    /// Subscribe to substrate System Events
    fn subscribe_events(
        &self,
    ) -> impl Future<
        Item = TypedSubscriptionStream<StorageChangeSet<<T as srml_system::Trait>::Hash>>,
        Error = RpcError,
    > {
        let events_key = b"System Events";
        let storage_key = twox_128(events_key);
        log::debug!("Events storage key {:?}", storage_key);

        self.state
            .subscribe_storage(Some(vec![StorageKey(storage_key.to_vec())]))
    }

    /// Submit an extrinsic, waiting for it to be finalized.
    /// If successful, returns the block hash.
    fn submit_and_watch(
        self,
        extrinsic: UncheckedExtrinsic,
    ) -> impl Future<Item = <T as srml_system::Trait>::Hash, Error = Error> {
        self.author
            .watch_extrinsic(extrinsic.encode().into())
            .map_err(Into::into)
            .and_then(|stream| {
                stream
                    .filter_map(|status| {
                        match status {
                            Status::Future | Status::Ready | Status::Broadcast(_) => None, // ignore in progress extrinsic for now
                            Status::Finalized(block_hash) => Some(Ok(block_hash)),
                            Status::Usurped(_) => Some(Err("Extrinsic Usurped".into())),
                            Status::Dropped => Some(Err("Extrinsic Dropped".into())),
                            Status::Invalid => Some(Err("Extrinsic Invalid".into())),
                        }
                    })
                    .into_future()
                    .map_err(|(e, _)| e.into())
                    .and_then(|(result, _)| {
                        result
                            .ok_or(Error::from("Stream terminated"))
                            .and_then(|r| r)
                            .into_future()
                    })
            })
    }

    /// Create and submit an extrinsic and return corresponding Event if successful
    pub fn create_and_submit_extrinsic<P>(
        self,
        signer: P,
        call: Call,
    ) -> impl Future<Item = ExtrinsicSuccess<T>, Error = Error>
    where
		P: Pair,
        <P as Pair>::Public: Into<<T as srml_system::Trait>::AccountId>,
    {
        let account_nonce = self
            .fetch_nonce(&signer.public().into())
            .map_err(Into::into);
        let genesis_hash = self
            .fetch_genesis_hash()
            .map_err(Into::into)
            .and_then(|genesis_hash| {
                future::result(genesis_hash.ok_or("Genesis hash not found".into()))
            });
        let events = self.subscribe_events().map_err(Into::into);

        account_nonce
            .join3(genesis_hash, events)
            .and_then(move |(index, genesis_hash, events)| {
                let extrinsic =
                    create_and_sign_extrinsic::<T, P>(index, call, genesis_hash, &signer);
                let ext_hash = <T as srml_system::Trait>::Hashing::hash_of(&extrinsic);

                log::info!("Submitting Extrinsic `{:?}`", ext_hash);

                let chain = self.chain.clone();
                self.submit_and_watch(extrinsic)
                    .and_then(move |bh| {
                        log::info!("Fetching block {:?}", bh);
                        chain
                            .block(Some(bh))
                            .map(move |b| (bh, b))
                            .map_err(Into::into)
                    })
                    .and_then(|(h, b)| {
                        b.ok_or(format!("Failed to find block {:?}", h).into())
                            .map(|b| (h, b))
                            .into_future()
                    })
                    .and_then(move |(bh, sb)| {
                        log::info!(
                            "Found block {:?}, with {} extrinsics",
                            bh,
                            sb.block.extrinsics.len()
                        );
                        wait_for_block_events::<T>(ext_hash, &sb, bh, events)
                    })
            })
    }
}

/// Creates and signs an Extrinsic for the supplied `Call`
fn create_and_sign_extrinsic<T, P>(
    index: <T as srml_system::Trait>::Index,
    function: Call,
    genesis_hash: <T as srml_system::Trait>::Hash,
    signer: &P,
) -> UncheckedExtrinsic
where
    T: srml_system::Trait,
    P: Pair,
{
    log::info!(
        "Creating Extrinsic with genesis hash {:?} and account nonce {:?}",
        genesis_hash,
        index
    );

    let extra = |nonce, fee| {
        (
            srml_system::CheckGenesis::<T>::new(),
            srml_system::CheckEra::<T>::from(Era::Immortal),
            srml_system::CheckNonce::<T>::from(nonce),
            srml_system::CheckWeight::<T>::new(),
            srml_balances::TakeFees::<T>::from(fee),
        )
    };

    let raw_payload = (function, extra(index, 0), genesis_hash);
    let signature = raw_payload.using_encoded(|payload| {
        if payload.len() > 256 {
            signer.sign(&blake2_256(payload)[..])
        } else {
            signer.sign(payload)
        }
    });

    UncheckedExtrinsic::new_signed(
        raw_payload.0,
        signer.public().into(),
        signature.into(),
        extra(index, 0),
    )
}

/// Waits for events for the block triggered by the extrinsic
fn wait_for_block_events<T>(
    ext_hash: <T as srml_system::Trait>::Hash,
    signed_block: &SignedBlock,
    block_hash: <T as srml_system::Trait>::Hash,
    events: TypedSubscriptionStream<StorageChangeSet<<T as srml_system::Trait>::Hash>>,
) -> impl Future<Item = ExtrinsicSuccess<T>, Error = Error>
where
    T: srml_system::Trait,
{
    let ext_index = signed_block
        .block
        .extrinsics
        .iter()
        .position(|ext| {
            let hash = <T as srml_system::Trait>::Hashing::hash_of(ext);
            hash == ext_hash
        })
        .ok_or(format!("Failed to find Extrinsic with hash {:?}", ext_hash).into())
        .into_future();

    let block_hash = block_hash.clone();
    let block_events = events
        .map(|event| {
            let records = event
                .changes
                .iter()
                .filter_map(|(_key, data)| {
                    data.as_ref()
                        .and_then(|data| Decode::decode(&mut &data.0[..]))
                })
                .flat_map(
                    |events: Vec<
                        srml_system::EventRecord<
                            <T as srml_system::Trait>::Event,
                            <T as srml_system::Trait>::Hash,
                        >,
                    >| events,
                )
                .collect::<Vec<_>>();
            log::debug!("Block {:?}, Events {:?}", event.block, records.len());
            (event.block, records)
        })
        .filter(move |(event_block, _)| *event_block == block_hash)
        .into_future()
        .map_err(|(e, _)| e.into())
        .map(|(events, _)| events);

    block_events
        .join(ext_index)
        .map(move |(events, ext_index)| {
            let events: Vec<<T as srml_system::Trait>::Event> = events
                .iter()
                .flat_map(|(_, events)| events)
                .filter_map(|e| {
                    if let srml_system::Phase::ApplyExtrinsic(i) = e.phase {
                        if i as usize == ext_index {
                            Some(e.event.clone())
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                })
                .collect::<Vec<<T as srml_system::Trait>::Event>>();
            ExtrinsicSuccess {
                block: block_hash,
                extrinsic: ext_hash,
                events,
            }
        })
}