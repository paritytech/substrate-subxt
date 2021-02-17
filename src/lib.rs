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

//! A library to **sub**mit e**xt**rinsics to a
//! [substrate](https://github.com/paritytech/substrate) node via RPC.

#![deny(
    bad_style,
    const_err,
    improper_ctypes,
    missing_docs,
    non_shorthand_field_patterns,
    no_mangle_generic_items,
    overflowing_literals,
    path_statements,
    patterns_in_fns_without_body,
    private_in_public,
    unconditional_recursion,
    unused_allocation,
    unused_comparisons,
    unused_parens,
    while_true,
    trivial_casts,
    trivial_numeric_casts,
    unused_extern_crates,
    clippy::all
)]
#![allow(clippy::type_complexity)]

#[macro_use]
extern crate substrate_subxt_proc_macro;

#[cfg(feature = "client")]
pub use substrate_subxt_client as client;

pub use sp_core;
pub use sp_runtime;

use codec::Decode;
use core::convert::Into;
use futures::future;
use jsonrpsee::client::Subscription;
use sp_core::{
    storage::{
        StorageChangeSet,
        StorageData,
        StorageKey,
    },
    Bytes,
};
pub use sp_runtime::traits::SignedExtension;
use sp_runtime::{
    generic::Era,
    traits::{
        Block,
        Header,
    },
    SaturatedConversion,
};
pub use sp_version::RuntimeVersion;
use std::marker::PhantomData;

mod error;
mod events;
pub mod extrinsic;
mod frame;
mod metadata;
mod rpc;
mod runtimes;
mod subscription;

pub use crate::{
    error::Error,
    events::{
        EventsDecoder,
        RawEvent,
    },
    extrinsic::{
        PairSigner,
        SignedExtra,
        Signer,
        UncheckedExtrinsic,
    },
    frame::*,
    metadata::{
        Metadata,
        MetadataError,
    },
    rpc::{
        BlockNumber,
        ExtrinsicSuccess,
        ReadProof,
        SystemProperties,
    },
    runtimes::*,
    subscription::*,
    substrate_subxt_proc_macro::*,
};
use crate::{
    frame::system::{
        AccountStoreExt,
        Phase,
        System,
    },
    rpc::{
        ChainBlock,
        Rpc,
    },
};

/// The period of validity of a transaction
#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
pub enum MortalPeriod {
    /// Create a mortal transaction with the specified period
    Mortal(u64),
    /// Create an immortal transaction
    Immortal,
}

/// ClientBuilder for constructing a Client.
#[derive(Default)]
pub struct ClientBuilder<T: Runtime> {
    _marker: std::marker::PhantomData<T>,
    url: Option<String>,
    client: Option<jsonrpsee::Client>,
    page_size: Option<u32>,
    mortal_period: Option<MortalPeriod>,
}

impl<T: Runtime> ClientBuilder<T> {
    /// Creates a new ClientBuilder.
    pub fn new() -> Self {
        Self {
            _marker: std::marker::PhantomData,
            url: None,
            client: None,
            page_size: None,
            mortal_period: None,
        }
    }

    /// Sets the jsonrpsee client.
    pub fn set_client<P: Into<jsonrpsee::Client>>(mut self, client: P) -> Self {
        self.client = Some(client.into());
        self
    }

    /// Set the substrate rpc address.
    pub fn set_url<P: Into<String>>(mut self, url: P) -> Self {
        self.url = Some(url.into());
        self
    }

    /// Set the page size.
    pub fn set_page_size(mut self, size: u32) -> Self {
        self.page_size = Some(size);
        self
    }

    /// Set the mortal period. Must be set if `Metadata::derive_mortal_period` results in an error.
    pub fn set_mortal_period(mut self, mortal_period: MortalPeriod) -> Self {
        self.mortal_period = Some(mortal_period);
        self
    }

    /// Creates a new Client.
    pub async fn build(self) -> Result<Client<T>, Error> {
        let client = if let Some(client) = self.client {
            client
        } else {
            let url = self.url.as_deref().unwrap_or("ws://127.0.0.1:9944");
            if url.starts_with("ws://") || url.starts_with("wss://") {
                jsonrpsee::ws_client(url).await?
            } else {
                jsonrpsee::http_client(url)
            }
        };
        let rpc = Rpc::new(client);
        let (metadata, genesis_hash, runtime_version, properties) = future::join4(
            rpc.metadata(),
            rpc.genesis_hash(),
            rpc.runtime_version(None),
            rpc.system_properties(),
        )
        .await;
        let metadata = metadata?;
        let mortal_period = if let Some(mortal_period) = self.mortal_period {
            mortal_period
        } else {
            match metadata.derive_mortal_period() {
                Err(e) => {
                    log::error!("`Metadata::derive_mortal_period` failed: {}. You may need to set `mortal_period` prior to invoking `Client::build`", e);
                    return Err(e.into())
                }
                Ok(period) => MortalPeriod::Mortal(period),
            }
        };

        Ok(Client {
            rpc,
            genesis_hash: genesis_hash?,
            metadata,
            properties: properties.unwrap_or_else(|_| Default::default()),
            runtime_version: runtime_version?,
            _marker: PhantomData,
            page_size: self.page_size.unwrap_or(10),
            signed_options: SigningOptions { mortal_period },
        })
    }
}

/// Construction options for a signed extrinsic
#[derive(Copy, Clone)]
struct SigningOptions {
    /// The period of validity, measured in blocks, that an extrinsic will live for, starting from a checkpoint
    /// block.
    ///
    /// Substrate reference:
    /// https://docs.rs/sp-runtime/2.0.0/sp_runtime/generic/enum.Era.html#variant.Mortal
    pub(crate) mortal_period: MortalPeriod,
}

/// Client to interface with a substrate node.
pub struct Client<T: Runtime> {
    rpc: Rpc<T>,
    genesis_hash: T::Hash,
    metadata: Metadata,
    properties: SystemProperties,
    runtime_version: RuntimeVersion,
    _marker: PhantomData<(fn() -> T::Signature, T::Extra)>,
    page_size: u32,
    signed_options: SigningOptions,
}

impl<T: Runtime> Clone for Client<T> {
    fn clone(&self) -> Self {
        Self {
            rpc: self.rpc.clone(),
            genesis_hash: self.genesis_hash,
            metadata: self.metadata.clone(),
            properties: self.properties.clone(),
            runtime_version: self.runtime_version.clone(),
            _marker: PhantomData,
            page_size: self.page_size,
            signed_options: self.signed_options,
        }
    }
}

/// Iterates over key value pairs in a map.
pub struct KeyIter<T: Runtime, F: Store<T>> {
    client: Client<T>,
    _marker: PhantomData<F>,
    count: u32,
    hash: T::Hash,
    start_key: Option<StorageKey>,
    buffer: Vec<(StorageKey, StorageData)>,
}

impl<T: Runtime, F: Store<T>> KeyIter<T, F> {
    /// Returns the next key value pair from a map.
    pub async fn next(&mut self) -> Result<Option<(StorageKey, F::Returns)>, Error> {
        loop {
            if let Some((k, v)) = self.buffer.pop() {
                return Ok(Some((k, Decode::decode(&mut &v.0[..])?)))
            } else {
                let keys = self
                    .client
                    .fetch_keys::<F>(self.count, self.start_key.take(), Some(self.hash))
                    .await?;

                if keys.is_empty() {
                    return Ok(None)
                }

                self.start_key = keys.last().cloned();

                let change_sets = self
                    .client
                    .rpc
                    .query_storage_at(&keys, Some(self.hash))
                    .await?;
                for change_set in change_sets {
                    for (k, v) in change_set.changes {
                        if let Some(v) = v {
                            self.buffer.push((k, v));
                        }
                    }
                }
                debug_assert_eq!(self.buffer.len(), keys.len());
            }
        }
    }
}

impl<T: Runtime> Client<T> {
    /// Returns the genesis hash.
    pub fn genesis(&self) -> &T::Hash {
        &self.genesis_hash
    }

    /// Returns the chain metadata.
    pub fn metadata(&self) -> &Metadata {
        &self.metadata
    }

    /// Returns the system properties
    pub fn properties(&self) -> &SystemProperties {
        &self.properties
    }

    /// Returns the current mortal_period
    pub fn mortal_period(&self) -> &MortalPeriod {
        &self.signed_options.mortal_period
    }

    /// Set the mortal period
    pub fn set_mortal_period(mut self, mortal_period: MortalPeriod) -> Self {
        self.signed_options.mortal_period = mortal_period;
        self
    }

    /// Fetch the value under an unhashed storage key
    pub async fn fetch_unhashed<V: Decode>(
        &self,
        key: StorageKey,
        hash: Option<T::Hash>,
    ) -> Result<Option<V>, Error> {
        if let Some(data) = self.rpc.storage(&key, hash).await? {
            Ok(Some(Decode::decode(&mut &data.0[..])?))
        } else {
            Ok(None)
        }
    }

    /// Fetch a StorageKey with an optional block hash.
    pub async fn fetch<F: Store<T>>(
        &self,
        store: &F,
        hash: Option<T::Hash>,
    ) -> Result<Option<F::Returns>, Error> {
        let key = store.key(&self.metadata)?;
        self.fetch_unhashed::<F::Returns>(key, hash).await
    }

    /// Fetch a StorageKey that has a default value with an optional block hash.
    pub async fn fetch_or_default<F: Store<T>>(
        &self,
        store: &F,
        hash: Option<T::Hash>,
    ) -> Result<F::Returns, Error> {
        if let Some(data) = self.fetch(store, hash).await? {
            Ok(data)
        } else {
            Ok(store.default(&self.metadata)?)
        }
    }

    /// Returns an iterator of key value pairs.
    pub async fn iter<F: Store<T>>(
        &self,
        hash: Option<T::Hash>,
    ) -> Result<KeyIter<T, F>, Error> {
        let hash = if let Some(hash) = hash {
            hash
        } else {
            self.block_hash(None)
                .await?
                .expect("didn't pass a block number; qed")
        };
        Ok(KeyIter {
            client: self.clone(),
            hash,
            count: self.page_size,
            start_key: None,
            buffer: Default::default(),
            _marker: PhantomData,
        })
    }

    /// Fetch up to `count` keys for a storage map in lexicographic order.
    ///
    /// Supports pagination by passing a value to `start_key`.
    pub async fn fetch_keys<F: Store<T>>(
        &self,
        count: u32,
        start_key: Option<StorageKey>,
        hash: Option<T::Hash>,
    ) -> Result<Vec<StorageKey>, Error> {
        let prefix = <F as Store<T>>::prefix(&self.metadata)?;
        let keys = self
            .rpc
            .storage_keys_paged(Some(prefix), count, start_key, hash)
            .await?;
        Ok(keys)
    }

    /// Query historical storage entries
    pub async fn query_storage(
        &self,
        keys: Vec<StorageKey>,
        from: T::Hash,
        to: Option<T::Hash>,
    ) -> Result<Vec<StorageChangeSet<<T as System>::Hash>>, Error> {
        self.rpc.query_storage(keys, from, to).await
    }

    /// Get a header
    pub async fn header<H>(&self, hash: Option<H>) -> Result<Option<T::Header>, Error>
    where
        H: Into<T::Hash> + 'static,
    {
        let header = self.rpc.header(hash.map(|h| h.into())).await?;
        Ok(header)
    }

    /// Get a block hash. By default returns the latest block hash
    pub async fn block_hash(
        &self,
        block_number: Option<BlockNumber>,
    ) -> Result<Option<T::Hash>, Error> {
        let hash = self.rpc.block_hash(block_number).await?;
        Ok(hash)
    }

    /// Get a block hash of the latest finalized block
    pub async fn finalized_head(&self) -> Result<T::Hash, Error> {
        let head = self.rpc.finalized_head().await?;
        Ok(head)
    }

    /// Get a block
    pub async fn block<H>(&self, hash: Option<H>) -> Result<Option<ChainBlock<T>>, Error>
    where
        H: Into<T::Hash> + 'static,
    {
        let block = self.rpc.block(hash.map(|h| h.into())).await?;
        Ok(block)
    }

    /// Get proof of storage entries at a specific block's state.
    pub async fn read_proof<H>(
        &self,
        keys: Vec<StorageKey>,
        hash: Option<H>,
    ) -> Result<ReadProof<T::Hash>, Error>
    where
        H: Into<T::Hash> + 'static,
    {
        let proof = self.rpc.read_proof(keys, hash.map(|h| h.into())).await?;
        Ok(proof)
    }

    /// Subscribe to events.
    pub async fn subscribe_events(
        &self,
    ) -> Result<Subscription<StorageChangeSet<T::Hash>>, Error> {
        let events = self.rpc.subscribe_events().await?;
        Ok(events)
    }

    /// Subscribe to new blocks.
    pub async fn subscribe_blocks(&self) -> Result<Subscription<T::Header>, Error> {
        let headers = self.rpc.subscribe_blocks().await?;
        Ok(headers)
    }

    /// Subscribe to finalized blocks.
    pub async fn subscribe_finalized_blocks(
        &self,
    ) -> Result<Subscription<T::Header>, Error> {
        let headers = self.rpc.subscribe_finalized_blocks().await?;
        Ok(headers)
    }

    /// Encodes a call.
    pub fn encode<C: Call<T>>(&self, call: C) -> Result<Encoded, Error> {
        Ok(self
            .metadata()
            .module_with_calls(C::MODULE)
            .and_then(|module| module.call(C::FUNCTION, call))?)
    }

    /// Creates an unsigned extrinsic.
    pub fn create_unsigned<C: Call<T> + Send + Sync>(
        &self,
        call: C,
    ) -> Result<UncheckedExtrinsic<T>, Error> {
        let call = self.encode(call)?;
        Ok(extrinsic::create_unsigned::<T>(call))
    }

    /// Creates a signed extrinsic.
    pub async fn create_signed<C: Call<T> + Send + Sync>(
        &self,
        call: C,
        signer: &(dyn Signer<T> + Send + Sync),
    ) -> Result<UncheckedExtrinsic<T>, Error>
    where
        <<T::Extra as SignedExtra<T>>::Extra as SignedExtension>::AdditionalSigned:
            Send + Sync,
    {
        let account_nonce = if let Some(nonce) = signer.nonce() {
            nonce
        } else {
            self.account(signer.account_id(), None).await?.nonce
        };
        let call = self.encode(call)?;
        let era_info = if let MortalPeriod::Mortal(mortal_period) =
            self.signed_options.mortal_period
        {
            let current_block = match self.block(None::<T::Hash>).await? {
                Some(signed_block) => signed_block.block,
                None => return Err("RPC chain_getBlock returned None when Some(signed_block) was expected".into()),
            };
            let current_number =
                (*current_block.header().number()).saturated_into::<u64>();
            let current_hash = current_block.hash();

            (
                Era::mortal(mortal_period, current_number),
                Some(current_hash),
            )
        } else {
            (Era::Immortal, None)
        };
        let signed = extrinsic::create_signed(
            &self.runtime_version,
            self.genesis_hash,
            account_nonce,
            call,
            signer,
            era_info,
        )
        .await?;
        Ok(signed)
    }

    /// Returns an events decoder for a call.
    pub fn events_decoder<C: Call<T>>(&self) -> EventsDecoder<T> {
        let metadata = self.metadata().clone();
        let mut decoder = EventsDecoder::new(metadata);
        C::events_decoder(&mut decoder);
        decoder
    }

    /// Create and submit an extrinsic and return corresponding Hash if successful
    pub async fn submit_extrinsic(
        &self,
        extrinsic: UncheckedExtrinsic<T>,
    ) -> Result<T::Hash, Error> {
        self.rpc.submit_extrinsic(extrinsic).await
    }

    /// Create and submit an extrinsic and return corresponding Event if successful
    pub async fn submit_and_watch_extrinsic(
        &self,
        extrinsic: UncheckedExtrinsic<T>,
        decoder: EventsDecoder<T>,
    ) -> Result<ExtrinsicSuccess<T>, Error> {
        self.rpc
            .submit_and_watch_extrinsic(extrinsic, decoder)
            .await
    }

    /// Submits a transaction to the chain.
    pub async fn submit<C: Call<T> + Send + Sync>(
        &self,
        call: C,
        signer: &(dyn Signer<T> + Send + Sync),
    ) -> Result<T::Hash, Error>
    where
        <<T::Extra as SignedExtra<T>>::Extra as SignedExtension>::AdditionalSigned:
            Send + Sync,
    {
        let extrinsic = self.create_signed(call, signer).await?;
        self.submit_extrinsic(extrinsic).await
    }

    /// Submits transaction to the chain and watch for events.
    pub async fn watch<C: Call<T> + Send + Sync>(
        &self,
        call: C,
        signer: &(dyn Signer<T> + Send + Sync),
    ) -> Result<ExtrinsicSuccess<T>, Error>
    where
        <<T::Extra as SignedExtra<T>>::Extra as SignedExtension>::AdditionalSigned:
            Send + Sync,
    {
        let extrinsic = self.create_signed(call, signer).await?;
        let decoder = self.events_decoder::<C>();
        self.submit_and_watch_extrinsic(extrinsic, decoder).await
    }

    /// Insert a key into the keystore.
    pub async fn insert_key(
        &self,
        key_type: String,
        suri: String,
        public: Bytes,
    ) -> Result<(), Error> {
        self.rpc.insert_key(key_type, suri, public).await
    }

    /// Generate new session keys and returns the corresponding public keys.
    pub async fn rotate_keys(&self) -> Result<Bytes, Error> {
        self.rpc.rotate_keys().await
    }

    /// Checks if the keystore has private keys for the given session public keys.
    ///
    /// `session_keys` is the SCALE encoded session keys object from the runtime.
    ///
    /// Returns `true` iff all private keys could be found.
    pub async fn has_session_keys(&self, session_keys: Bytes) -> Result<bool, Error> {
        self.rpc.has_session_keys(session_keys).await
    }

    /// Checks if the keystore has private keys for the given public key and key type.
    ///
    /// Returns `true` if a private key could be found.
    pub async fn has_key(
        &self,
        public_key: Bytes,
        key_type: String,
    ) -> Result<bool, Error> {
        self.rpc.has_key(public_key, key_type).await
    }
}

/// Wraps an already encoded byte vector, prevents being encoded as a raw byte vector as part of
/// the transaction payload
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Encoded(pub Vec<u8>);

impl codec::Encode for Encoded {
    fn encode(&self) -> Vec<u8> {
        self.0.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sp_core::storage::{
        well_known_keys,
        StorageKey,
    };
    use sp_keyring::AccountKeyring;
    use substrate_subxt_client::{
        DatabaseConfig,
        KeystoreConfig,
        Role,
        SubxtClient,
        SubxtClientConfig,
    };
    use tempdir::TempDir;

    pub(crate) type TestRuntime = crate::NodeTemplateRuntime;

    pub(crate) async fn test_client_with(
        key: AccountKeyring,
    ) -> (Client<TestRuntime>, TempDir) {
        env_logger::try_init().ok();
        let tmp = TempDir::new("subxt-").expect("failed to create tempdir");
        let config = SubxtClientConfig {
            impl_name: "substrate-subxt-full-client",
            impl_version: "0.0.1",
            author: "substrate subxt",
            copyright_start_year: 2020,
            db: DatabaseConfig::RocksDb {
                path: tmp.path().join("db"),
                cache_size: 128,
            },
            keystore: KeystoreConfig::Path {
                path: tmp.path().join("keystore"),
                password: None,
            },
            chain_spec: test_node::chain_spec::development_config().unwrap(),
            role: Role::Authority(key),
            telemetry: None,
        };
        let client = ClientBuilder::new()
            .set_client(
                SubxtClient::from_config(config, test_node::service::new_full)
                    .expect("Error creating subxt client"),
            )
            .set_page_size(3)
            .build()
            .await
            .expect("Error creating client");
        (client, tmp)
    }

    pub(crate) async fn test_client() -> (Client<TestRuntime>, TempDir) {
        test_client_with(AccountKeyring::Alice).await
    }

    #[async_std::test]
    async fn test_insert_key() {
        // Bob is not an authority, so block production should be disabled.
        let (client, _tmp) = test_client_with(AccountKeyring::Bob).await;
        let mut blocks = client.subscribe_blocks().await.unwrap();
        // get the genesis block.
        assert_eq!(blocks.next().await.number, 0);
        let public = AccountKeyring::Alice.public().as_array_ref().to_vec();
        client
            .insert_key(
                "aura".to_string(),
                "//Alice".to_string(),
                public.clone().into(),
            )
            .await
            .unwrap();
        assert!(client
            .has_key(public.clone().into(), "aura".to_string())
            .await
            .unwrap());
        // Alice is an authority, so blocks should be produced.
        assert_eq!(blocks.next().await.number, 1);
    }

    #[async_std::test]
    async fn test_tx_transfer_balance() {
        let mut signer = PairSigner::new(AccountKeyring::Alice.pair());
        let dest = AccountKeyring::Bob.to_account_id().into();

        let (client, _) = test_client().await;
        let nonce = client
            .account(&AccountKeyring::Alice.to_account_id(), None)
            .await
            .unwrap()
            .nonce;
        signer.set_nonce(nonce);
        client
            .submit(
                balances::TransferCall {
                    to: &dest,
                    amount: 10_000,
                },
                &signer,
            )
            .await
            .unwrap();

        // check that nonce is handled correctly
        signer.increment_nonce();
        client
            .submit(
                balances::TransferCall {
                    to: &dest,
                    amount: 10_000,
                },
                &signer,
            )
            .await
            .unwrap();
    }

    #[async_std::test]
    async fn test_getting_hash() {
        let (client, _) = test_client().await;
        client.block_hash(None).await.unwrap();
    }

    #[async_std::test]
    async fn test_getting_block() {
        let (client, _) = test_client().await;
        let block_hash = client.block_hash(None).await.unwrap();
        client.block(block_hash).await.unwrap();
    }

    #[async_std::test]
    async fn test_getting_read_proof() {
        let (client, _) = test_client().await;
        let block_hash = client.block_hash(None).await.unwrap();
        client
            .read_proof(
                vec![
                    StorageKey(well_known_keys::HEAP_PAGES.to_vec()),
                    StorageKey(well_known_keys::EXTRINSIC_INDEX.to_vec()),
                ],
                block_hash,
            )
            .await
            .unwrap();
    }

    #[async_std::test]
    async fn test_chain_subscribe_blocks() {
        let (client, _) = test_client().await;
        let mut blocks = client.subscribe_blocks().await.unwrap();
        blocks.next().await;
    }

    #[async_std::test]
    async fn test_chain_subscribe_finalized_blocks() {
        let (client, _) = test_client().await;
        let mut blocks = client.subscribe_finalized_blocks().await.unwrap();
        blocks.next().await;
    }

    #[async_std::test]
    async fn test_fetch_keys() {
        let (client, _) = test_client().await;
        let keys = client
            .fetch_keys::<system::AccountStore<_>>(4, None, None)
            .await
            .unwrap();
        assert_eq!(keys.len(), 4)
    }

    #[async_std::test]
    async fn test_iter() {
        let (client, _) = test_client().await;
        let mut iter = client.iter::<system::AccountStore<_>>(None).await.unwrap();
        let mut i = 0;
        while let Some(_) = iter.next().await.unwrap() {
            i += 1;
        }
        assert_eq!(i, 4);
    }
}
