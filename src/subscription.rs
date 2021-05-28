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

use jsonrpsee_ws_client::Subscription;
use sp_core::{
    storage::{
        StorageChangeSet,
        StorageKey,
    },
    twox_128,
};
use sp_runtime::traits::Header;
use std::collections::VecDeque;

use crate::{
    error::Error,
    events::{
        EventsDecoder,
        Raw,
        RawEvent,
    },
    frame::{
        system::Phase,
        Event,
    },
    rpc::Rpc,
    runtimes::Runtime,
};

/// Event subscription simplifies filtering a storage change set stream for
/// events of interest.
pub struct EventSubscription<'a, T: Runtime> {
    subscription: EventStorageSubscription<T>,
    decoder: &'a EventsDecoder<T>,
    block: Option<T::Hash>,
    extrinsic: Option<usize>,
    event: Option<(&'static str, &'static str)>,
    events: VecDeque<RawEvent>,
    finished: bool,
}

impl<'a, T: Runtime> EventSubscription<'a, T> {
    /// Creates a new event subscription.
    pub fn new(
        subscription: EventStorageSubscription<T>,
        decoder: &'a EventsDecoder<T>,
    ) -> Self {
        Self {
            subscription,
            decoder,
            block: None,
            extrinsic: None,
            event: None,
            events: Default::default(),
            finished: false,
        }
    }

    /// Only returns events contained in the block with the given hash.
    pub fn filter_block(&mut self, block: T::Hash) {
        self.block = Some(block);
    }

    /// Only returns events from block emitted by extrinsic with index.
    pub fn filter_extrinsic(&mut self, block: T::Hash, ext_index: usize) {
        self.block = Some(block);
        self.extrinsic = Some(ext_index);
    }

    /// Filters events by type.
    pub fn filter_event<E: Event<T>>(&mut self) {
        self.event = Some((E::MODULE, E::EVENT));
    }

    /// Gets the next event.
    pub async fn next(&mut self) -> Option<Result<RawEvent, Error>> {
        loop {
            if let Some(event) = self.events.pop_front() {
                return Some(Ok(event))
            }
            if self.finished {
                return None
            }
            // always return None if subscription has closed
            let change_set = self.subscription.next().await?;
            if let Some(hash) = self.block.as_ref() {
                if &change_set.block == hash {
                    self.finished = true;
                } else {
                    continue
                }
            }
            for (_key, data) in change_set.changes {
                if let Some(data) = data {
                    let raw_events = match self.decoder.decode_events(&mut &data.0[..]) {
                        Ok(events) => events,
                        Err(error) => return Some(Err(error)),
                    };
                    for (phase, raw) in raw_events {
                        if let Phase::ApplyExtrinsic(i) = phase {
                            if let Some(ext_index) = self.extrinsic {
                                if i as usize != ext_index {
                                    continue
                                }
                            }
                        }
                        let event = match raw {
                            Raw::Event(event) => event,
                            Raw::Error(err) => return Some(Err(err.into())),
                        };
                        if let Some((module, variant)) = self.event {
                            if event.module != module || event.variant != variant {
                                continue
                            }
                        }
                        self.events.push_back(event);
                    }
                }
            }
        }
    }
}

pub(crate) struct SystemEvents(StorageKey);

impl SystemEvents {
    pub(crate) fn new() -> Self {
        let mut storage_key = twox_128(b"System").to_vec();
        storage_key.extend(twox_128(b"Events").to_vec());
        log::debug!("Events storage key {:?}", hex::encode(&storage_key));
        Self(StorageKey(storage_key))
    }
}

impl From<SystemEvents> for StorageKey {
    fn from(key: SystemEvents) -> Self {
        key.0
    }
}

/// Event subscription to only fetch finalized storage changes.
pub struct FinalizedEventStorageSubscription<T: Runtime> {
    rpc: Rpc<T>,
    subscription: Subscription<T::Header>,
    storage_changes: VecDeque<StorageChangeSet<T::Hash>>,
    storage_key: StorageKey,
}

impl<T: Runtime> FinalizedEventStorageSubscription<T> {
    /// Creates a new finalized event storage subscription.
    pub fn new(rpc: Rpc<T>, subscription: Subscription<T::Header>) -> Self {
        Self {
            rpc,
            subscription,
            storage_changes: Default::default(),
            storage_key: SystemEvents::new().into(),
        }
    }

    /// Gets the next change_set.
    pub async fn next(&mut self) -> Option<StorageChangeSet<T::Hash>> {
        loop {
            if let Some(storage_change) = self.storage_changes.pop_front() {
                return Some(storage_change)
            }
            let header: T::Header = self.subscription.next().await?;
            self.storage_changes.extend(
                self.rpc
                    .query_storage_at(&[self.storage_key.clone()], Some(header.hash()))
                    .await
                    .ok()?,
            );
        }
    }
}

/// Wrapper over imported and finalized event subscriptions.
pub enum EventStorageSubscription<T: Runtime> {
    /// Events that are InBlock
    Imported(Subscription<StorageChangeSet<T::Hash>>),
    /// Events that are Finalized
    Finalized(FinalizedEventStorageSubscription<T>),
}

impl<T: Runtime> EventStorageSubscription<T> {
    /// Gets the next change_set from the subscription.
    pub async fn next(&mut self) -> Option<StorageChangeSet<T::Hash>> {
        match self {
            Self::Imported(event_sub) => event_sub.next().await,
            Self::Finalized(event_sub) => event_sub.next().await,
        }
    }
}
