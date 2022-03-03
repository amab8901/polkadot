// Copyright 2020 Parity Technologies (UK) Ltd.
// This file is part of Polkadot.

// Polkadot is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

// Polkadot is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.

// You should have received a copy of the GNU General Public License
// along with Polkadot.  If not, see <http://www.gnu.org/licenses/>.

use crate::{barriers::AllowSubscriptionsFrom, test_utils::*};
pub use crate::{
	AllowKnownQueryResponses, AllowTopLevelPaidExecutionFrom, AllowUnpaidExecutionFrom,
	FixedRateOfFungible, FixedWeightBounds, LocationInverter, TakeWeightCredit,
};
pub use frame_support::{
	dispatch::{
		DispatchError, DispatchInfo, DispatchResultWithPostInfo, Dispatchable, Parameter, Weight,
	},
	ensure, parameter_types,
	sp_runtime::DispatchErrorWithPostInfo,
	traits::{Contains, Get, IsInVec},
	weights::{GetDispatchInfo, PostDispatchInfo},
};
pub use parity_scale_codec::{Decode, Encode};
pub use sp_std::{
	cell::RefCell,
	collections::{btree_map::BTreeMap, btree_set::BTreeSet},
	fmt::Debug,
	marker::PhantomData,
};
pub use xcm::latest::prelude::*;
pub use xcm_executor::{
	traits::{
		AssetExchange, AssetLock, ConvertOrigin, Enact, ExportXcm, FeeManager, FeeReason,
		FilterAssetLocation, LockError, OnResponse, TransactAsset, UniversalLocation,
	},
	Assets, Config,
};

pub enum TestOrigin {
	Root,
	Relay,
	Signed(u64),
	Parachain(u32),
}

/// A dummy call.
///
/// Each item contains the amount of weight that it *wants* to consume as the first item, and the actual amount (if
/// different from the former) in the second option.
#[derive(Debug, Encode, Decode, Eq, PartialEq, Clone, Copy, scale_info::TypeInfo)]
pub enum TestCall {
	OnlyRoot(Weight, Option<Weight>),
	OnlyParachain(Weight, Option<Weight>, Option<u32>),
	OnlySigned(Weight, Option<Weight>, Option<u64>),
	Any(Weight, Option<Weight>),
}
impl Dispatchable for TestCall {
	type Origin = TestOrigin;
	type Config = ();
	type Info = ();
	type PostInfo = PostDispatchInfo;
	fn dispatch(self, origin: Self::Origin) -> DispatchResultWithPostInfo {
		let mut post_info = PostDispatchInfo::default();
		post_info.actual_weight = match self {
			TestCall::OnlyRoot(_, maybe_actual) |
			TestCall::OnlySigned(_, maybe_actual, _) |
			TestCall::OnlyParachain(_, maybe_actual, _) |
			TestCall::Any(_, maybe_actual) => maybe_actual,
		};
		if match (&origin, &self) {
			(TestOrigin::Parachain(i), TestCall::OnlyParachain(_, _, Some(j))) => i == j,
			(TestOrigin::Signed(i), TestCall::OnlySigned(_, _, Some(j))) => i == j,
			(TestOrigin::Root, TestCall::OnlyRoot(..)) |
			(TestOrigin::Parachain(_), TestCall::OnlyParachain(_, _, None)) |
			(TestOrigin::Signed(_), TestCall::OnlySigned(_, _, None)) |
			(_, TestCall::Any(..)) => true,
			_ => false,
		} {
			Ok(post_info)
		} else {
			Err(DispatchErrorWithPostInfo { error: DispatchError::BadOrigin, post_info })
		}
	}
}

impl GetDispatchInfo for TestCall {
	fn get_dispatch_info(&self) -> DispatchInfo {
		let weight = *match self {
			TestCall::OnlyRoot(estimate, ..) |
			TestCall::OnlyParachain(estimate, ..) |
			TestCall::OnlySigned(estimate, ..) |
			TestCall::Any(estimate, ..) => estimate,
		};
		DispatchInfo { weight, ..Default::default() }
	}
}

thread_local! {
	pub static SENT_XCM: RefCell<Vec<(MultiLocation, Xcm<()>)>> = RefCell::new(Vec::new());
	pub static EXPORTED_XCM: RefCell<Vec<(NetworkId, u32, InteriorMultiLocation, Xcm<()>)>> = RefCell::new(Vec::new());
	pub static EXPORTER_OVERRIDE: RefCell<Option<(
		fn(NetworkId, u32, &InteriorMultiLocation, &Xcm<()>) -> Result<MultiAssets, SendError>,
		fn(NetworkId, u32, InteriorMultiLocation, Xcm<()>) -> Result<(), SendError>,
	)>> = RefCell::new(None);
	pub static SEND_PRICE: RefCell<MultiAssets> = RefCell::new(MultiAssets::new());
}
pub fn sent_xcm() -> Vec<(MultiLocation, opaque::Xcm)> {
	SENT_XCM.with(|q| (*q.borrow()).clone())
}
pub fn set_send_price(p: impl Into<MultiAsset>) {
	SEND_PRICE.with(|l| l.replace(p.into().into()));
}
pub fn exported_xcm() -> Vec<(NetworkId, u32, InteriorMultiLocation, opaque::Xcm)> {
	EXPORTED_XCM.with(|q| (*q.borrow()).clone())
}
pub fn set_exporter_override(
	price: fn(NetworkId, u32, &InteriorMultiLocation, &Xcm<()>) -> Result<MultiAssets, SendError>,
	deliver: fn(NetworkId, u32, InteriorMultiLocation, Xcm<()>) -> Result<(), SendError>,
) {
	EXPORTER_OVERRIDE.with(|x| x.replace(Some((price, deliver))));
}
#[allow(dead_code)]
pub fn clear_exporter_override() {
	EXPORTER_OVERRIDE.with(|x| x.replace(None));
}
pub struct TestMessageSender;
impl SendXcm for TestMessageSender {
	type Ticket = (MultiLocation, Xcm<()>);
	fn validate(
		dest: &mut Option<MultiLocation>,
		msg: &mut Option<Xcm<()>>,
	) -> SendResult<(MultiLocation, Xcm<()>)> {
		let pair = (dest.take().unwrap(), msg.take().unwrap());
		Ok((pair, SEND_PRICE.with(|l| l.borrow().clone())))
	}
	fn deliver(pair: (MultiLocation, Xcm<()>)) -> Result<(), SendError> {
		SENT_XCM.with(|q| q.borrow_mut().push(pair));
		Ok(())
	}
}
pub struct TestMessageExporter;
impl ExportXcm for TestMessageExporter {
	type Ticket = (NetworkId, u32, InteriorMultiLocation, Xcm<()>);
	fn validate(
		network: NetworkId,
		channel: u32,
		dest: &mut Option<InteriorMultiLocation>,
		msg: &mut Option<Xcm<()>>,
	) -> SendResult<(NetworkId, u32, InteriorMultiLocation, Xcm<()>)> {
		let (d, m) = (dest.take().unwrap(), msg.take().unwrap());
		let r: Result<MultiAssets, SendError> = EXPORTER_OVERRIDE.with(|e| {
			if let Some((ref f, _)) = &*e.borrow() {
				f(network, channel, &d, &m)
			} else {
				Ok(MultiAssets::new())
			}
		});
		match r {
			Ok(price) => Ok(((network, channel, d, m), price)),
			Err(e) => {
				*dest = Some(d);
				*msg = Some(m);
				Err(e)
			},
		}
	}
	fn deliver(tuple: (NetworkId, u32, InteriorMultiLocation, Xcm<()>)) -> Result<(), SendError> {
		EXPORTER_OVERRIDE.with(|e| {
			if let Some((_, ref f)) = &*e.borrow() {
				let (network, channel, dest, msg) = tuple;
				f(network, channel, dest, msg)
			} else {
				EXPORTED_XCM.with(|q| q.borrow_mut().push(tuple));
				Ok(())
			}
		})
	}
}

thread_local! {
	pub static ASSETS: RefCell<BTreeMap<MultiLocation, Assets>> = RefCell::new(BTreeMap::new());
}
pub fn assets(who: impl Into<MultiLocation>) -> Assets {
	ASSETS.with(|a| a.borrow().get(&who.into()).cloned()).unwrap_or_default()
}
pub fn asset_list(who: impl Into<MultiLocation>) -> Vec<MultiAsset> {
	MultiAssets::from(assets(who)).into_inner()
}
pub fn add_asset(who: impl Into<MultiLocation>, what: impl Into<MultiAsset>) {
	ASSETS.with(|a| a.borrow_mut().entry(who.into()).or_insert(Assets::new()).subsume(what.into()));
}

pub struct TestAssetTransactor;
impl TransactAsset for TestAssetTransactor {
	fn deposit_asset(what: &MultiAsset, who: &MultiLocation) -> Result<(), XcmError> {
		add_asset(who.clone(), what.clone());
		Ok(())
	}

	fn withdraw_asset(what: &MultiAsset, who: &MultiLocation) -> Result<Assets, XcmError> {
		ASSETS.with(|a| {
			a.borrow_mut()
				.get_mut(who)
				.ok_or(XcmError::NotWithdrawable)?
				.try_take(what.clone().into())
				.map_err(|_| XcmError::NotWithdrawable)
		})
	}
}

pub fn to_account(l: impl Into<MultiLocation>) -> Result<u64, MultiLocation> {
	Ok(match l.into() {
		// Siblings at 2000+id
		MultiLocation { parents: 1, interior: X1(Parachain(id)) } => 2000 + id as u64,
		// Accounts are their number
		MultiLocation { parents: 0, interior: X1(AccountIndex64 { index, .. }) } => index,
		// Children at 1000+id
		MultiLocation { parents: 0, interior: X1(Parachain(id)) } => 1000 + id as u64,
		// Self at 3000
		MultiLocation { parents: 0, interior: Here } => 3000,
		// Parent at 3001
		MultiLocation { parents: 1, interior: Here } => 3001,
		l => {
			// Is it a foreign-consensus?
			let uni = ExecutorUniversalLocation::get();
			if l.parents as usize != uni.len() {
				return Err(l)
			}
			match l.first_interior() {
				Some(GlobalConsensus(Kusama)) => 4000,
				Some(GlobalConsensus(Polkadot)) => 4001,
				_ => return Err(l),
			}
		},
	})
}

pub struct TestOriginConverter;
impl ConvertOrigin<TestOrigin> for TestOriginConverter {
	fn convert_origin(
		origin: impl Into<MultiLocation>,
		kind: OriginKind,
	) -> Result<TestOrigin, MultiLocation> {
		use OriginKind::*;
		match (kind, origin.into()) {
			(Superuser, _) => Ok(TestOrigin::Root),
			(SovereignAccount, l) => Ok(TestOrigin::Signed(to_account(l)?)),
			(Native, MultiLocation { parents: 0, interior: X1(Parachain(id)) }) =>
				Ok(TestOrigin::Parachain(id)),
			(Native, MultiLocation { parents: 1, interior: Here }) => Ok(TestOrigin::Relay),
			(Native, MultiLocation { parents: 0, interior: X1(AccountIndex64 { index, .. }) }) =>
				Ok(TestOrigin::Signed(index)),
			(_, origin) => Err(origin),
		}
	}
}

thread_local! {
	pub static IS_RESERVE: RefCell<BTreeMap<MultiLocation, Vec<MultiAssetFilter>>> = RefCell::new(BTreeMap::new());
	pub static IS_TELEPORTER: RefCell<BTreeMap<MultiLocation, Vec<MultiAssetFilter>>> = RefCell::new(BTreeMap::new());
	pub static UNIVERSAL_ALIASES: RefCell<BTreeSet<(MultiLocation, Junction)>> = RefCell::new(BTreeSet::new());
}
pub fn add_reserve(from: MultiLocation, asset: MultiAssetFilter) {
	IS_RESERVE.with(|r| r.borrow_mut().entry(from).or_default().push(asset));
}
#[allow(dead_code)]
pub fn add_teleporter(from: MultiLocation, asset: MultiAssetFilter) {
	IS_TELEPORTER.with(|r| r.borrow_mut().entry(from).or_default().push(asset));
}
pub fn add_universal_alias(bridge: impl Into<MultiLocation>, consensus: impl Into<Junction>) {
	UNIVERSAL_ALIASES.with(|r| r.borrow_mut().insert((bridge.into(), consensus.into())));
}
pub fn clear_universal_aliases() {
	UNIVERSAL_ALIASES.with(|r| r.replace(Default::default()));
}

pub struct TestIsReserve;
impl FilterAssetLocation for TestIsReserve {
	fn filter_asset_location(asset: &MultiAsset, origin: &MultiLocation) -> bool {
		IS_RESERVE
			.with(|r| r.borrow().get(origin).map_or(false, |v| v.iter().any(|a| a.matches(asset))))
	}
}
pub struct TestIsTeleporter;
impl FilterAssetLocation for TestIsTeleporter {
	fn filter_asset_location(asset: &MultiAsset, origin: &MultiLocation) -> bool {
		IS_TELEPORTER
			.with(|r| r.borrow().get(origin).map_or(false, |v| v.iter().any(|a| a.matches(asset))))
	}
}

pub struct TestUniversalAliases;
impl Contains<(MultiLocation, Junction)> for TestUniversalAliases {
	fn contains(t: &(MultiLocation, Junction)) -> bool {
		UNIVERSAL_ALIASES.with(|r| r.borrow().contains(t))
	}
}

pub enum ResponseSlot {
	Expecting(MultiLocation),
	Received(Response),
}
thread_local! {
	pub static QUERIES: RefCell<BTreeMap<u64, ResponseSlot>> = RefCell::new(BTreeMap::new());
}
pub struct TestResponseHandler;
impl OnResponse for TestResponseHandler {
	fn expecting_response(
		origin: &MultiLocation,
		query_id: u64,
		_querier: Option<&MultiLocation>,
	) -> bool {
		QUERIES.with(|q| match q.borrow().get(&query_id) {
			Some(ResponseSlot::Expecting(ref l)) => l == origin,
			_ => false,
		})
	}
	fn on_response(
		_origin: &MultiLocation,
		query_id: u64,
		_querier: Option<&MultiLocation>,
		response: xcm::latest::Response,
		_max_weight: Weight,
	) -> Weight {
		QUERIES.with(|q| {
			q.borrow_mut().entry(query_id).and_modify(|v| {
				if matches!(*v, ResponseSlot::Expecting(..)) {
					*v = ResponseSlot::Received(response);
				}
			});
		});
		10
	}
}
pub fn expect_response(query_id: u64, from: MultiLocation) {
	QUERIES.with(|q| q.borrow_mut().insert(query_id, ResponseSlot::Expecting(from)));
}
pub fn response(query_id: u64) -> Option<Response> {
	QUERIES.with(|q| {
		q.borrow().get(&query_id).and_then(|v| match v {
			ResponseSlot::Received(r) => Some(r.clone()),
			_ => None,
		})
	})
}

parameter_types! {
	pub static ExecutorUniversalLocation: InteriorMultiLocation
		= (ByGenesis([0; 32]), Parachain(42)).into();
	pub UnitWeightCost: Weight = 10;
}
parameter_types! {
	// Nothing is allowed to be paid/unpaid by default.
	pub static AllowUnpaidFrom: Vec<MultiLocation> = vec![];
	pub static AllowPaidFrom: Vec<MultiLocation> = vec![];
	pub static AllowSubsFrom: Vec<MultiLocation> = vec![];
	// 1_000_000_000_000 => 1 unit of asset for 1 unit of Weight.
	pub static WeightPrice: (AssetId, u128) = (From::from(Here), 1_000_000_000_000);
	pub static MaxInstructions: u32 = 100;
}

pub type TestBarrier = (
	TakeWeightCredit,
	AllowKnownQueryResponses<TestResponseHandler>,
	AllowTopLevelPaidExecutionFrom<IsInVec<AllowPaidFrom>>,
	AllowUnpaidExecutionFrom<IsInVec<AllowUnpaidFrom>>,
	AllowSubscriptionsFrom<IsInVec<AllowSubsFrom>>,
);

thread_local! {
	pub static IS_WAIVED: RefCell<Vec<FeeReason>> = RefCell::new(vec![]);
}
#[allow(dead_code)]
pub fn set_fee_waiver(waived: Vec<FeeReason>) {
	IS_WAIVED.with(|l| l.replace(waived));
}

pub struct TestFeeManager;
impl FeeManager for TestFeeManager {
	fn is_waived(_: Option<&MultiLocation>, r: FeeReason) -> bool {
		IS_WAIVED.with(|l| l.borrow().contains(&r))
	}
	fn handle_fee(_: MultiAssets) {}
}

#[derive(Clone, Eq, PartialEq, Debug)]
pub enum LockTraceItem {
	Lock { unlocker: MultiLocation, asset: MultiAsset, owner: MultiLocation },
	Unlock { unlocker: MultiLocation, asset: MultiAsset, owner: MultiLocation },
	Note { locker: MultiLocation, asset: MultiAsset, owner: MultiLocation },
	Reduce { locker: MultiLocation, asset: MultiAsset, owner: MultiLocation },
}
thread_local! {
	pub static NEXT_INDEX: RefCell<u32> = RefCell::new(0);
	pub static LOCK_TRACE: RefCell<Vec<LockTraceItem>> = RefCell::new(Vec::new());
	pub static ALLOWED_UNLOCKS: RefCell<BTreeMap<(MultiLocation, MultiLocation), Assets>> = RefCell::new(BTreeMap::new());
	pub static ALLOWED_REQUEST_UNLOCKS: RefCell<BTreeMap<(MultiLocation, MultiLocation), Assets>> = RefCell::new(BTreeMap::new());
}

pub fn take_lock_trace() -> Vec<LockTraceItem> {
	LOCK_TRACE.with(|l| l.replace(Vec::new()))
}
pub fn allow_unlock(
	unlocker: impl Into<MultiLocation>,
	asset: impl Into<MultiAsset>,
	owner: impl Into<MultiLocation>,
) {
	ALLOWED_UNLOCKS.with(|l| {
		l.borrow_mut()
			.entry((owner.into(), unlocker.into()))
			.or_default()
			.subsume(asset.into())
	});
}
pub fn disallow_unlock(
	unlocker: impl Into<MultiLocation>,
	asset: impl Into<MultiAsset>,
	owner: impl Into<MultiLocation>,
) {
	ALLOWED_UNLOCKS.with(|l| {
		l.borrow_mut()
			.entry((owner.into(), unlocker.into()))
			.or_default()
			.saturating_take(asset.into().into())
	});
}
pub fn unlock_allowed(unlocker: &MultiLocation, asset: &MultiAsset, owner: &MultiLocation) -> bool {
	ALLOWED_UNLOCKS.with(|l| {
		l.borrow_mut()
			.get(&(owner.clone(), unlocker.clone()))
			.map_or(false, |x| x.contains_asset(asset))
	})
}
pub fn allow_request_unlock(
	locker: impl Into<MultiLocation>,
	asset: impl Into<MultiAsset>,
	owner: impl Into<MultiLocation>,
) {
	ALLOWED_REQUEST_UNLOCKS.with(|l| {
		l.borrow_mut()
			.entry((owner.into(), locker.into()))
			.or_default()
			.subsume(asset.into())
	});
}
pub fn disallow_request_unlock(
	locker: impl Into<MultiLocation>,
	asset: impl Into<MultiAsset>,
	owner: impl Into<MultiLocation>,
) {
	ALLOWED_REQUEST_UNLOCKS.with(|l| {
		l.borrow_mut()
			.entry((owner.into(), locker.into()))
			.or_default()
			.saturating_take(asset.into().into())
	});
}
pub fn request_unlock_allowed(
	locker: &MultiLocation,
	asset: &MultiAsset,
	owner: &MultiLocation,
) -> bool {
	ALLOWED_REQUEST_UNLOCKS.with(|l| {
		l.borrow_mut()
			.get(&(owner.clone(), locker.clone()))
			.map_or(false, |x| x.contains_asset(asset))
	})
}

pub struct TestTicket(LockTraceItem);
impl Enact for TestTicket {
	fn enact(self) -> Result<(), LockError> {
		match &self.0 {
			LockTraceItem::Lock { unlocker, asset, owner } =>
				allow_unlock(unlocker.clone(), asset.clone(), owner.clone()),
			LockTraceItem::Unlock { unlocker, asset, owner } =>
				disallow_unlock(unlocker.clone(), asset.clone(), owner.clone()),
			LockTraceItem::Reduce { locker, asset, owner } =>
				disallow_request_unlock(locker.clone(), asset.clone(), owner.clone()),
			_ => {},
		}
		LOCK_TRACE.with(move |l| l.borrow_mut().push(self.0));
		Ok(())
	}
}

pub struct TestAssetLock;
impl AssetLock for TestAssetLock {
	type LockTicket = TestTicket;
	type UnlockTicket = TestTicket;
	type ReduceTicket = TestTicket;

	fn prepare_lock(
		unlocker: MultiLocation,
		asset: MultiAsset,
		owner: MultiLocation,
	) -> Result<Self::LockTicket, LockError> {
		ensure!(assets(owner.clone()).contains_asset(&asset), LockError::AssetNotOwned);
		Ok(TestTicket(LockTraceItem::Lock { unlocker, asset, owner }))
	}

	fn prepare_unlock(
		unlocker: MultiLocation,
		asset: MultiAsset,
		owner: MultiLocation,
	) -> Result<Self::UnlockTicket, LockError> {
		ensure!(unlock_allowed(&unlocker, &asset, &owner), LockError::NotLocked);
		Ok(TestTicket(LockTraceItem::Unlock { unlocker, asset, owner }))
	}

	fn note_unlockable(
		locker: MultiLocation,
		asset: MultiAsset,
		owner: MultiLocation,
	) -> Result<(), LockError> {
		allow_request_unlock(locker.clone(), asset.clone(), owner.clone());
		let item = LockTraceItem::Note { locker, asset, owner };
		LOCK_TRACE.with(move |l| l.borrow_mut().push(item));
		Ok(())
	}

	fn prepare_reduce_unlockable(
		locker: MultiLocation,
		asset: MultiAsset,
		owner: MultiLocation,
	) -> Result<Self::ReduceTicket, xcm_executor::traits::LockError> {
		ensure!(request_unlock_allowed(&locker, &asset, &owner), LockError::NotLocked);
		Ok(TestTicket(LockTraceItem::Reduce { locker, asset, owner }))
	}
}

thread_local! {
	pub static EXCHANGE_ASSETS: RefCell<Assets> = RefCell::new(Assets::new());
}
pub fn set_exchange_assets(assets: impl Into<MultiAssets>) {
	EXCHANGE_ASSETS.with(|a| a.replace(assets.into().into()));
}
pub fn exchange_assets() -> MultiAssets {
	EXCHANGE_ASSETS.with(|a| a.borrow().clone().into())
}
pub struct TestAssetExchange;
impl AssetExchange for TestAssetExchange {
	fn exchange_asset(
		_origin: Option<&MultiLocation>,
		give: Assets,
		want: &MultiAssets,
		maximal: bool,
	) -> Result<Assets, Assets> {
		let mut have = EXCHANGE_ASSETS.with(|l| l.borrow().clone());
		ensure!(have.contains_assets(want), give);
		let get = if maximal {
			std::mem::replace(&mut have, Assets::new())
		} else {
			have.saturating_take(want.clone().into())
		};
		have.subsume_assets(give);
		EXCHANGE_ASSETS.with(|l| l.replace(have));
		Ok(get)
	}
}

pub struct TestConfig;
impl Config for TestConfig {
	type Call = TestCall;
	type XcmSender = TestMessageSender;
	type AssetTransactor = TestAssetTransactor;
	type OriginConverter = TestOriginConverter;
	type IsReserve = TestIsReserve;
	type IsTeleporter = TestIsTeleporter;
	type LocationInverter = LocationInverter<ExecutorUniversalLocation>;
	type Barrier = TestBarrier;
	type Weigher = FixedWeightBounds<UnitWeightCost, TestCall, MaxInstructions>;
	type Trader = FixedRateOfFungible<WeightPrice, ()>;
	type ResponseHandler = TestResponseHandler;
	type AssetTrap = TestAssetTrap;
	type AssetLocker = TestAssetLock;
	type AssetExchanger = TestAssetExchange;
	type AssetClaims = TestAssetTrap;
	type SubscriptionService = TestSubscriptionService;
	type PalletInstancesInfo = TestPalletsInfo;
	type MaxAssetsIntoHolding = MaxAssetsIntoHolding;
	type FeeManager = TestFeeManager;
	type UniversalAliases = TestUniversalAliases;
	type MessageExporter = TestMessageExporter;
}

pub fn fungible_multi_asset(location: MultiLocation, amount: u128) -> MultiAsset {
	(AssetId::from(location), Fungibility::Fungible(amount)).into()
}