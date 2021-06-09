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

extern crate proc_macro;

mod call;
mod event;
mod module;
mod store;
mod test;
mod utils;

use proc_macro::TokenStream;
use proc_macro_error::proc_macro_error;
use synstructure::{
    decl_derive,
    Structure,
};

/// Register type sizes for [EventsDecoder](struct.EventsDecoder.html) and set the `MODULE`.
///
/// The `module` macro registers the type sizes of the associated types of a trait so that [EventsDecoder](struct.EventsDecoder.html)
/// can decode events of that type when received from Substrate. It also sets the `MODULE` constant
/// to the name of the trait (must match the name of the Substrate pallet) that enables the [Call](), [Event]() and [Store]() macros to work.
///
/// If you do not want an associated type to be registered, likely because you never expect it as part of a response payload to be decoded, use `#[module(ignore)]` on the type.
///
/// Example:
///
/// ```ignore
/// #[module]
/// pub trait Herd: Husbandry {
///     type Hooves: HoofCounter;
///     type Wool: WoollyAnimal;
///     #[module(ignore)]
///     type Digestion: EnergyProducer + std::fmt::Debug;
/// }
/// ```
///
/// The above will produce the following code:
///
/// ```ignore
/// pub trait Herd: Husbandry {
///     type Hooves: HoofCounter;
///     type Wool: WoollyAnimal;
///     #[module(ignore)]
///     type Digestion: EnergyProducer + std::fmt::Debug;
/// }
///
/// const MODULE: &str = "Herd";
///
/// // `EventTypeRegistry` extension trait.
/// pub trait HerdEventTypeRegistry {
///     // Registers this modules types.
///     fn with_herd(&mut self);
/// }
///
/// impl<T: Herd + Runtime> EventTypeRegistry for
///     substrate_subxt::EventTypeRegistry<T>
/// {
///     fn with_herd(&mut self) {
///         self.register_type_size::<T::Hooves>("Hooves");
///         self.register_type_size::<T::Wool>("Wool");
///     }
/// }
/// ```
///
/// The following type sizes are registered by default: `bool, u8, u32, AccountId, AccountIndex,
/// AuthorityId, AuthorityIndex, AuthorityWeight, BlockNumber, DispatchInfo, Hash, Kind,
/// MemberCount, PhantomData, PropIndex, ProposalIndex, ReferendumIndex, SessionIndex, VoteThreshold`
#[proc_macro_attribute]
#[proc_macro_error]
pub fn module(args: TokenStream, input: TokenStream) -> TokenStream {
    module::module(args.into(), input.into()).into()
}

decl_derive!(
    [Call] =>
    /// Derive macro that implements [substrate_subxt::Call](../substrate_subxt/trait.Call.html) for your struct
    /// and defines&implements the calls as an extension trait.
    ///
    /// Use the `Call` derive macro in tandem with the [#module](../substrate_subxt/attr.module.html) macro to extend
    /// your struct to enable calls to substrate and to decode events. The struct maps to the corresponding Substrate runtime call, e.g.:
    ///
    /// ```ignore
    /// decl_module! {
    ///     /* … */
    ///     pub fn fun_stuff(origin, something: Vec<u8>) -> DispatchResult { /* … */ }
    ///     /* … */
    /// }
    ///```
    ///
    /// Implements [substrate_subxt::Call](../substrate_subxt/trait.Call.html) and adds an extension trait that
    /// provides two methods named as your struct.
    ///
    /// Example:
    /// ```rust,ignore
    /// pub struct MyRuntime;
    ///
    /// impl System for MyRuntime { /* … */ }
    /// impl Balances for MyRuntime { /* … */ }
    ///
    /// #[module]
    /// pub trait MyTrait: System + Balances {}
    ///
    /// #[derive(Call, Encode)]
    /// pub struct FunStuffCall<T: MyTrait> {
    ///     /// Runtime marker.
    ///     pub _runtime: PhantomData<T>,
    ///     /// The argument passed to the call..
    ///     pub something: Vec<u8>,
    /// }
    /// ```
    ///
    /// When building a [Client](../substrate_subxt/struct.Client.html) parameterised to `MyRuntime`, you have access to
    /// two new methods: `fun_stuff()` and `fun_stuff_and_watch()` by way of the derived `FunStuffExt`
    /// trait. The `_and_watch` variant makes the call and waits for the result. The fields of the
    /// input struct become arguments to the calls (ignoring the marker field).
    ///
    /// Under the hood the implementation calls [submit()](../substrate_subxt/struct.Client.html#method.submit) and
    /// [watch()](../substrate_subxt/struct.Client.html#method.watch) respectively.
    ///
    /// *N.B.* You must use the `#[derive(Call)]` macro with `#[module]` in the same module or you will get errors
    /// about undefined method with a name starting with `with_`.

    #[proc_macro_error] call
);
fn call(s: Structure) -> TokenStream {
    call::call(s).into()
}

decl_derive!([Event] => #[proc_macro_error] event);
fn event(s: Structure) -> TokenStream {
    event::event(s).into()
}

decl_derive!([Store, attributes(store)] => #[proc_macro_error] store);
fn store(s: Structure) -> TokenStream {
    store::store(s).into()
}

#[proc_macro]
#[proc_macro_error]
pub fn subxt_test(input: TokenStream) -> TokenStream {
    test::test(input.into()).into()
}
