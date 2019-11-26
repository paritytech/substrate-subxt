// Copyright 2019 Parity Technologies (UK) Ltd.
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

use std::{
    collections::HashMap,
    convert::TryFrom,
    marker::PhantomData,
    str::FromStr,
};

use parity_scale_codec::{
    Decode,
    Encode,
};

use runtime_metadata::{
    DecodeDifferent,
    RuntimeMetadata,
    RuntimeMetadataPrefixed,
    StorageEntryModifier,
    StorageEntryType,
    StorageHasher,
    META_RESERVED,
};
use substrate_primitives::storage::StorageKey;

use crate::codec::Encoded;

#[derive(Debug, thiserror::Error)]
pub enum MetadataError {
    #[error("Module not found")]
    ModuleNotFound(String),
    #[error("Call not found")]
    CallNotFound(&'static str),
    #[error("Event not found")]
    EventNotFound(u8),
    #[error("Storage not found")]
    StorageNotFound(&'static str),
    #[error("Storage type error")]
    StorageTypeError,
    #[error("Map value type error")]
    MapValueTypeError,
    #[error("Index not found")]
    IndexNotFound(String),
}

#[derive(Clone, Debug)]
pub struct Metadata {
    modules: HashMap<String, ModuleMetadata>,
}

impl Metadata {
    pub fn modules(&self) -> impl Iterator<Item = &ModuleMetadata> {
        self.modules.values()
    }

    pub fn module<S>(&self, name: S) -> Result<&ModuleMetadata, MetadataError>
    where
        S: ToString,
    {
        let name = name.to_string();
        self.modules
            .get(&name)
            .ok_or(MetadataError::ModuleNotFound(name))
    }

    pub fn module_name(
        &self,
        module_index: u8,
    ) -> Result<&ModuleMetadata, MetadataError> {
        self.modules
            .values()
            .find(|&module| module.index_for_events == Some(module_index))
            .ok_or(MetadataError::EventNotFound(module_index))
    }

    pub fn pretty(&self) -> String {
        let mut string = String::new();
        for (name, module) in &self.modules {
            string.push_str(name.as_str());
            string.push('\n');
            for storage in module.storage.keys() {
                string.push_str(" s  ");
                string.push_str(storage.as_str());
                string.push('\n');
            }
            for call in module.calls.keys() {
                string.push_str(" c  ");
                string.push_str(call.as_str());
                string.push('\n');
            }
            for event in module.events.values() {
                string.push_str(" e  ");
                string.push_str(event.name.as_str());
                string.push('\n');
            }
        }
        string
    }
}

#[derive(Clone, Debug)]
pub struct ModuleMetadata {
    index_for_calls: Option<u8>,
    index_for_events: Option<u8>,
    name: String,
    storage: HashMap<String, StorageMetadata>,
    calls: HashMap<String, Vec<u8>>,
    events: HashMap<u8, ModuleEventMetadata>,
    // constants
}

impl ModuleMetadata {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn call<T: Encode>(
        &self,
        function: &'static str,
        params: T,
    ) -> Result<Encoded, MetadataError> {
        let index = self
            .index_for_calls
            .ok_or(MetadataError::IndexNotFound(self.name.clone()))?;
        let fn_bytes = self
            .calls
            .get(function)
            .ok_or(MetadataError::CallNotFound(function))?;
        let mut bytes = vec![index];
        bytes.extend(fn_bytes);
        bytes.extend(params.encode());
        Ok(Encoded(bytes))
    }

    pub fn storage(&self, key: &'static str) -> Result<&StorageMetadata, MetadataError> {
        self.storage
            .get(key)
            .ok_or(MetadataError::StorageNotFound(key))
    }

    pub fn events(&self) -> impl Iterator<Item = &ModuleEventMetadata> {
        self.events.values()
    }

    pub fn event(&self, index: u8) -> Result<&ModuleEventMetadata, MetadataError> {
        self.events
            .get(&index)
            .ok_or(MetadataError::EventNotFound(index))
    }
}

#[derive(Clone, Debug)]
pub struct StorageMetadata {
    prefix: String,
    modifier: StorageEntryModifier,
    ty: StorageEntryType,
    default: Vec<u8>,
}

impl StorageMetadata {
    pub fn get_map<K: Encode, V: Decode + Clone>(
        &self,
    ) -> Result<StorageMap<K, V>, MetadataError> {
        match &self.ty {
            StorageEntryType::Map { hasher, .. } => {
                let prefix = self.prefix.as_bytes().to_vec();
                let hasher = hasher.to_owned();
                let default = Decode::decode(&mut &self.default[..])
                    .map_err(|_| MetadataError::MapValueTypeError)?;
                Ok(StorageMap {
                    _marker: PhantomData,
                    prefix,
                    hasher,
                    default,
                })
            }
            _ => Err(MetadataError::StorageTypeError),
        }
    }
}

#[derive(Clone, Debug)]
pub struct StorageMap<K, V> {
    _marker: PhantomData<K>,
    prefix: Vec<u8>,
    hasher: StorageHasher,
    default: V,
}

impl<K: Encode, V: Decode + Clone> StorageMap<K, V> {
    pub fn key(&self, key: K) -> StorageKey {
        let mut bytes = self.prefix.clone();
        bytes.extend(key.encode());
        let hash = match self.hasher {
            StorageHasher::Blake2_128 => {
                substrate_primitives::blake2_128(&bytes).to_vec()
            }
            StorageHasher::Blake2_256 => {
                substrate_primitives::blake2_256(&bytes).to_vec()
            }
            StorageHasher::Twox128 => substrate_primitives::twox_128(&bytes).to_vec(),
            StorageHasher::Twox256 => substrate_primitives::twox_256(&bytes).to_vec(),
            StorageHasher::Twox64Concat => substrate_primitives::twox_64(&bytes).to_vec(),
        };
        StorageKey(hash)
    }

    pub fn default(&self) -> V {
        self.default.clone()
    }
}

#[derive(Clone, Debug)]
pub struct ModuleEventMetadata {
    pub name: String,
    arguments: Vec<EventArg>,
}

impl ModuleEventMetadata {
    pub fn arguments(&self) -> Vec<EventArg> {
        self.arguments.to_vec()
    }
}

/// Naive representation of event argument types, supports current set of substrate EventArg types.
/// If and when Substrate uses `type-metadata`, this can be replaced.
///
/// Used to calculate the size of a instance of an event variant without having the concrete type,
/// so the raw bytes can be extracted from the encoded `Vec<EventRecord<E>>` (without `E` defined).
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub enum EventArg {
    Primitive(String),
    Vec(Box<EventArg>),
    Tuple(Vec<EventArg>),
}

impl FromStr for EventArg {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.starts_with("Vec<") {
            if s.ends_with('>') {
                Ok(EventArg::Vec(Box::new(s[4..s.len() - 1].parse()?)))
            } else {
                Err(Error::InvalidEventArg(
                    s.to_string(),
                    "Expected closing `>` for `Vec`",
                ))
            }
        } else if s.starts_with('(') {
            if s.ends_with(')') {
                let mut args = Vec::new();
                for arg in s[1..s.len() - 1].split(',') {
                    let arg = arg.trim().parse()?;
                    args.push(arg)
                }
                Ok(EventArg::Tuple(args))
            } else {
                Err(Error::InvalidEventArg(
                    s.to_string(),
                    "Expecting closing `)` for tuple",
                ))
            }
        } else {
            Ok(EventArg::Primitive(s.to_string()))
        }
    }
}

impl EventArg {
    /// Returns all primitive types for this EventArg
    pub fn primitives(&self) -> Vec<String> {
        match self {
            EventArg::Primitive(p) => vec![p.clone()],
            EventArg::Vec(arg) => arg.primitives(),
            EventArg::Tuple(args) => {
                let mut primitives = Vec::new();
                for arg in args {
                    primitives.extend(arg.primitives())
                }
                primitives
            }
        }
    }
}

#[derive(Debug)]
pub enum Error {
    InvalidPrefix,
    InvalidVersion,
    ExpectedDecoded,
    InvalidEventArg(String, &'static str),
}

impl TryFrom<RuntimeMetadataPrefixed> for Metadata {
    type Error = Error;

    fn try_from(metadata: RuntimeMetadataPrefixed) -> Result<Self, Self::Error> {
        if metadata.0 != META_RESERVED {
            return Err(Error::InvalidPrefix)
        }
        let meta = match metadata.1 {
            RuntimeMetadata::V8(meta) => meta,
            _ => return Err(Error::InvalidVersion),
        };
        let mut modules = HashMap::new();
        let mut call_index = 0;
        let mut event_index = 0;
        for module in convert(meta.modules)?.into_iter() {
            let module_name = convert(module.name.clone())?;
            let mut index_for_calls = None;
            let mut index_for_events = None;
            if module.calls.is_some() {
                index_for_calls = Some(call_index);
                call_index += 1;
            }
            if module.event.is_some() {
                index_for_events = Some(event_index);
                event_index += 1;
            }
            let module_metadata =
                convert_module(index_for_calls, index_for_events, module)?;
            modules.insert(module_name, module_metadata);
        }
        Ok(Metadata { modules })
    }
}

fn convert<B: 'static, O: 'static>(dd: DecodeDifferent<B, O>) -> Result<O, Error> {
    match dd {
        DecodeDifferent::Decoded(value) => Ok(value),
        _ => Err(Error::ExpectedDecoded),
    }
}

fn convert_module(
    index_for_calls: Option<u8>,
    index_for_events: Option<u8>,
    module: runtime_metadata::ModuleMetadata,
) -> Result<ModuleMetadata, Error> {
    let mut storage_map = HashMap::new();
    if let Some(storage) = module.storage {
        let storage = convert(storage)?;
        let prefix = convert(storage.prefix)?;
        for entry in convert(storage.entries)?.into_iter() {
            let entry_name = convert(entry.name.clone())?;
            let entry_prefix = format!("{} {}", prefix, entry_name);
            let entry = convert_entry(entry_prefix, entry)?;
            storage_map.insert(entry_name, entry);
        }
    }
    let mut call_map = HashMap::new();
    if let Some(calls) = module.calls {
        for (index, call) in convert(calls)?.into_iter().enumerate() {
            let name = convert(call.name)?;
            call_map.insert(name, vec![index as u8]);
        }
    }
    let mut event_map = HashMap::new();
    if let Some(events) = module.event {
        for (index, event) in convert(events)?.into_iter().enumerate() {
            event_map.insert(index as u8, convert_event(event)?);
        }
    }
    Ok(ModuleMetadata {
        index_for_calls: index_for_calls,
        index_for_events: index_for_events,
        name: convert(module.name)?,
        storage: storage_map,
        calls: call_map,
        events: event_map,
    })
}

fn convert_event(
    event: runtime_metadata::EventMetadata,
) -> Result<ModuleEventMetadata, Error> {
    let name = convert(event.name)?;
    let mut arguments = Vec::new();
    for arg in convert(event.arguments)? {
        let arg = arg.parse::<EventArg>()?;
        arguments.push(arg);
    }
    Ok(ModuleEventMetadata { name, arguments })
}

fn convert_entry(
    prefix: String,
    entry: runtime_metadata::StorageEntryMetadata,
) -> Result<StorageMetadata, Error> {
    let default = convert(entry.default)?;
    Ok(StorageMetadata {
        prefix,
        modifier: entry.modifier,
        ty: entry.ty,
        default,
    })
}
