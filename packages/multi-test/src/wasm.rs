use schemars::JsonSchema;
use serde::de::DeserializeOwned;
use std::collections::HashMap;
use std::fmt;
use std::ops::Deref;

use cosmwasm_std::{
    from_slice, Addr, Api, Binary, BlockInfo, ContractInfo, CosmosMsg, Deps, DepsMut, Empty, Env,
    MessageInfo, Querier, QuerierWrapper, Reply, Response, Storage, SubMsg, WasmQuery,
};

use crate::transactions::{RepLog, StorageTransaction};

/// Interface to call into a Contract
pub trait Contract<T>
where
    T: Clone + fmt::Debug + PartialEq + JsonSchema,
{
    fn execute(
        &self,
        deps: DepsMut,
        env: Env,
        info: MessageInfo,
        msg: Vec<u8>,
    ) -> Result<Response<T>, String>;

    fn instantiate(
        &self,
        deps: DepsMut,
        env: Env,
        info: MessageInfo,
        msg: Vec<u8>,
    ) -> Result<Response<T>, String>;

    fn sudo(&self, deps: DepsMut, env: Env, msg: Vec<u8>) -> Result<Response<T>, String>;

    fn reply(&self, deps: DepsMut, env: Env, msg: Reply) -> Result<Response<T>, String>;

    fn query(&self, deps: Deps, env: Env, msg: Vec<u8>) -> Result<Binary, String>;
}

type ContractFn<T, C, E> =
    fn(deps: DepsMut, env: Env, info: MessageInfo, msg: T) -> Result<Response<C>, E>;
type SudoFn<T, C, E> = fn(deps: DepsMut, env: Env, msg: T) -> Result<Response<C>, E>;
type ReplyFn<C, E> = fn(deps: DepsMut, env: Env, msg: Reply) -> Result<Response<C>, E>;
type QueryFn<T, E> = fn(deps: Deps, env: Env, msg: T) -> Result<Binary, E>;

type ContractClosure<T, C, E> = Box<dyn Fn(DepsMut, Env, MessageInfo, T) -> Result<Response<C>, E>>;
type SudoClosure<T, C, E> = Box<dyn Fn(DepsMut, Env, T) -> Result<Response<C>, E>>;
type ReplyClosure<C, E> = Box<dyn Fn(DepsMut, Env, Reply) -> Result<Response<C>, E>>;
type QueryClosure<T, E> = Box<dyn Fn(Deps, Env, T) -> Result<Binary, E>>;

/// Wraps the exported functions from a contract and provides the normalized format
/// Place T4 and E4 at the end, as we just want default placeholders for most contracts that don't have sudo
pub struct ContractWrapper<T1, T2, T3, E1, E2, E3, C = Empty, T4 = Empty, E4 = String, E5 = String>
where
    T1: DeserializeOwned,
    T2: DeserializeOwned,
    T3: DeserializeOwned,
    T4: DeserializeOwned,
    E1: ToString,
    E2: ToString,
    E3: ToString,
    E4: ToString,
    E5: ToString,
    C: Clone + fmt::Debug + PartialEq + JsonSchema,
{
    execute_fn: ContractClosure<T1, C, E1>,
    instantiate_fn: ContractClosure<T2, C, E2>,
    query_fn: QueryClosure<T3, E3>,
    sudo_fn: Option<SudoClosure<T4, C, E4>>,
    reply_fn: Option<ReplyClosure<C, E5>>,
}

impl<T1, T2, T3, E1, E2, E3, C> ContractWrapper<T1, T2, T3, E1, E2, E3, C>
where
    T1: DeserializeOwned + 'static,
    T2: DeserializeOwned + 'static,
    T3: DeserializeOwned + 'static,
    E1: ToString + 'static,
    E2: ToString + 'static,
    E3: ToString + 'static,
    C: Clone + fmt::Debug + PartialEq + JsonSchema + 'static,
{
    pub fn new(
        execute_fn: ContractFn<T1, C, E1>,
        instantiate_fn: ContractFn<T2, C, E2>,
        query_fn: QueryFn<T3, E3>,
    ) -> Self {
        ContractWrapper {
            execute_fn: Box::new(execute_fn),
            instantiate_fn: Box::new(instantiate_fn),
            query_fn: Box::new(query_fn),
            sudo_fn: None,
            reply_fn: None,
        }
    }

    /// this will take a contract that returns Response<Empty> and will "upgrade" it
    /// to Response<C> if needed to be compatible with a chain-specific extension
    pub fn new_with_empty(
        execute_fn: ContractFn<T1, Empty, E1>,
        instantiate_fn: ContractFn<T2, Empty, E2>,
        query_fn: QueryFn<T3, E3>,
    ) -> Self {
        ContractWrapper {
            execute_fn: customize_fn(execute_fn),
            instantiate_fn: customize_fn(instantiate_fn),
            query_fn: Box::new(query_fn),
            sudo_fn: None,
            reply_fn: None,
        }
    }
}

impl<T1, T2, T3, E1, E2, E3, C, T4, E4> ContractWrapper<T1, T2, T3, E1, E2, E3, C, T4, E4>
where
    T1: DeserializeOwned + 'static,
    T2: DeserializeOwned + 'static,
    T3: DeserializeOwned + 'static,
    T4: DeserializeOwned + 'static,
    E1: ToString + 'static,
    E2: ToString + 'static,
    E3: ToString + 'static,
    E4: ToString + 'static,
    C: Clone + fmt::Debug + PartialEq + JsonSchema + 'static,
{
    pub fn new_with_sudo(
        execute_fn: ContractFn<T1, C, E1>,
        instantiate_fn: ContractFn<T2, C, E2>,
        query_fn: QueryFn<T3, E3>,
        sudo_fn: SudoFn<T4, C, E4>,
    ) -> Self {
        ContractWrapper {
            execute_fn: Box::new(execute_fn),
            instantiate_fn: Box::new(instantiate_fn),
            query_fn: Box::new(query_fn),
            sudo_fn: Some(Box::new(sudo_fn)),
            reply_fn: None,
        }
    }
}

impl<T1, T2, T3, E1, E2, E3, C, E5> ContractWrapper<T1, T2, T3, E1, E2, E3, C, Empty, String, E5>
where
    T1: DeserializeOwned + 'static,
    T2: DeserializeOwned + 'static,
    T3: DeserializeOwned + 'static,
    E1: ToString + 'static,
    E2: ToString + 'static,
    E3: ToString + 'static,
    E5: ToString + 'static,
    C: Clone + fmt::Debug + PartialEq + JsonSchema + 'static,
{
    pub fn new_with_reply(
        execute_fn: ContractFn<T1, C, E1>,
        instantiate_fn: ContractFn<T2, C, E2>,
        query_fn: QueryFn<T3, E3>,
        reply_fn: ReplyFn<C, E5>,
    ) -> Self {
        ContractWrapper {
            execute_fn: Box::new(execute_fn),
            instantiate_fn: Box::new(instantiate_fn),
            query_fn: Box::new(query_fn),
            sudo_fn: None,
            reply_fn: Some(Box::new(reply_fn)),
        }
    }
}

fn customize_fn<T, C, E>(raw_fn: ContractFn<T, Empty, E>) -> ContractClosure<T, C, E>
where
    T: DeserializeOwned + 'static,
    E: ToString + 'static,
    C: Clone + fmt::Debug + PartialEq + JsonSchema + 'static,
{
    let customized =
        move |deps: DepsMut, env: Env, info: MessageInfo, msg: T| -> Result<Response<C>, E> {
            raw_fn(deps, env, info, msg).map(customize_response::<C>)
        };
    Box::new(customized)
}

fn customize_response<C>(resp: Response<Empty>) -> Response<C>
where
    C: Clone + fmt::Debug + PartialEq + JsonSchema,
{
    Response::<C> {
        messages: resp.messages.into_iter().map(customize_msg::<C>).collect(),
        events: resp.events,
        attributes: resp.attributes,
        data: resp.data,
    }
}

fn customize_msg<C>(msg: SubMsg<Empty>) -> SubMsg<C>
where
    C: Clone + fmt::Debug + PartialEq + JsonSchema,
{
    SubMsg {
        msg: match msg.msg {
            CosmosMsg::Wasm(wasm) => CosmosMsg::Wasm(wasm),
            CosmosMsg::Bank(bank) => CosmosMsg::Bank(bank),
            CosmosMsg::Staking(staking) => CosmosMsg::Staking(staking),
            CosmosMsg::Custom(_) => unreachable!(),
            #[cfg(feature = "stargate")]
            CosmosMsg::Ibc(ibc) => CosmosMsg::Ibc(ibc),
            #[cfg(feature = "stargate")]
            CosmosMsg::Stargate { type_url, value } => CosmosMsg::Stargate { type_url, value },
            _ => panic!("unknown message variant {:?}", msg),
        },
        id: msg.id,
        gas_limit: msg.gas_limit,
        reply_on: msg.reply_on,
    }
}

impl<T1, T2, T3, E1, E2, E3, C, T4, E4, E5> Contract<C>
    for ContractWrapper<T1, T2, T3, E1, E2, E3, C, T4, E4, E5>
where
    T1: DeserializeOwned,
    T2: DeserializeOwned,
    T3: DeserializeOwned,
    T4: DeserializeOwned,
    E1: ToString,
    E2: ToString,
    E3: ToString,
    E4: ToString,
    E5: ToString,
    C: Clone + fmt::Debug + PartialEq + JsonSchema,
{
    fn execute(
        &self,
        deps: DepsMut,
        env: Env,
        info: MessageInfo,
        msg: Vec<u8>,
    ) -> Result<Response<C>, String> {
        let msg: T1 = from_slice(&msg).map_err(|e| e.to_string())?;
        let res = (self.execute_fn)(deps, env, info, msg);
        res.map_err(|e| e.to_string())
    }

    fn instantiate(
        &self,
        deps: DepsMut,
        env: Env,
        info: MessageInfo,
        msg: Vec<u8>,
    ) -> Result<Response<C>, String> {
        let msg: T2 = from_slice(&msg).map_err(|e| e.to_string())?;
        let res = (self.instantiate_fn)(deps, env, info, msg);
        res.map_err(|e| e.to_string())
    }

    // this returns an error if the contract doesn't implement sudo
    fn sudo(&self, deps: DepsMut, env: Env, msg: Vec<u8>) -> Result<Response<C>, String> {
        let msg: T4 = from_slice(&msg).map_err(|e| e.to_string())?;
        let res = match &self.sudo_fn {
            Some(sudo) => sudo(deps, env, msg),
            None => return Err("sudo not implemented for contract".to_string()),
        };
        res.map_err(|e| e.to_string())
    }

    // this returns an error if the contract doesn't implement sudo
    fn reply(&self, deps: DepsMut, env: Env, reply_data: Reply) -> Result<Response<C>, String> {
        let res = match &self.reply_fn {
            Some(reply) => reply(deps, env, reply_data),
            None => return Err("reply not implemented for contract".to_string()),
        };
        res.map_err(|e| e.to_string())
    }

    fn query(&self, deps: Deps, env: Env, msg: Vec<u8>) -> Result<Binary, String> {
        let msg: T3 = from_slice(&msg).map_err(|e| e.to_string())?;
        let res = (self.query_fn)(deps, env, msg);
        res.map_err(|e| e.to_string())
    }
}

struct ContractData {
    code_id: usize,
    storage: Box<dyn Storage>,
}

impl ContractData {
    fn new(code_id: usize, storage: Box<dyn Storage>) -> Self {
        ContractData { code_id, storage }
    }

    fn as_mut(&mut self) -> ContractMut {
        ContractMut {
            code_id: self.code_id,
            storage: self.storage.as_mut(),
        }
    }

    fn as_ref(&self) -> ContractRef {
        ContractRef {
            code_id: self.code_id,
            storage: self.storage.as_ref(),
        }
    }
}

struct ContractMut<'a> {
    code_id: usize,
    storage: &'a mut dyn Storage,
}

impl<'a> ContractMut<'a> {
    fn new(code_id: usize, storage: &'a mut dyn Storage) -> Self {
        ContractMut { code_id, storage }
    }
}

struct ContractRef<'a> {
    code_id: usize,
    storage: &'a dyn Storage,
}

impl<'a> ContractRef<'a> {
    fn new(code_id: usize, storage: &'a dyn Storage) -> Self {
        ContractRef { code_id, storage }
    }
}

pub fn next_block(block: &mut BlockInfo) {
    block.time = block.time.plus_seconds(5);
    block.height += 1;
}

pub type StorageFactory = fn() -> Box<dyn Storage>;

pub struct WasmRouter<C>
where
    C: Clone + fmt::Debug + PartialEq + JsonSchema,
{
    // WasmState - cache this, pass in separate?
    codes: HashMap<usize, Box<dyn Contract<C>>>,
    contracts: HashMap<Addr, ContractData>,
    // WasmConst
    block: BlockInfo,
    api: Box<dyn Api>,
    storage_factory: StorageFactory,
}

impl<C> WasmRouter<C>
where
    C: Clone + fmt::Debug + PartialEq + JsonSchema,
{
    pub fn new(api: Box<dyn Api>, block: BlockInfo, storage_factory: StorageFactory) -> Self {
        WasmRouter {
            codes: HashMap::new(),
            contracts: HashMap::new(),
            block,
            api,
            storage_factory,
        }
    }

    pub fn set_block(&mut self, block: BlockInfo) {
        self.block = block;
    }

    // this let's use use "next block" steps that add eg. one height and 5 seconds
    pub fn update_block<F: Fn(&mut BlockInfo)>(&mut self, action: F) {
        action(&mut self.block);
    }

    /// Returns a copy of the current block_info
    pub fn block_info(&self) -> BlockInfo {
        self.block.clone()
    }

    pub fn store_code(&mut self, code: Box<dyn Contract<C>>) -> usize {
        let idx = self.codes.len() + 1;
        self.codes.insert(idx, code);
        idx
    }

    pub fn cache(&'_ self) -> WasmCache<'_, C> {
        WasmCache::new(self)
    }

    pub fn query(&self, querier: &dyn Querier, request: WasmQuery) -> Result<Binary, String> {
        match request {
            WasmQuery::Smart { contract_addr, msg } => {
                self.query_smart(Addr::unchecked(contract_addr), querier, msg.into())
            }
            WasmQuery::Raw { contract_addr, key } => {
                self.query_raw(Addr::unchecked(contract_addr), &key)
            }
            q => panic!("Unsupported wasm query: {:?}", q),
        }
    }

    pub fn query_smart(
        &self,
        address: Addr,
        querier: &dyn Querier,
        msg: Vec<u8>,
    ) -> Result<Binary, String> {
        self.with_storage(querier, address, |handler, deps, env| {
            handler.query(deps, env, msg)
        })
    }

    pub fn query_raw(&self, address: Addr, key: &[u8]) -> Result<Binary, String> {
        let contract = self
            .contracts
            .get(&address)
            .ok_or_else(|| "Unregistered contract address".to_string())?;
        let data = contract.storage.get(&key).unwrap_or_default();
        Ok(data.into())
    }

    fn get_env<T: Into<Addr>>(&self, address: T) -> Env {
        Env {
            block: self.block.clone(),
            contract: ContractInfo {
                address: address.into(),
            },
        }
    }

    fn with_storage<F, T>(
        &self,
        querier: &dyn Querier,
        address: Addr,
        action: F,
    ) -> Result<T, String>
    where
        F: FnOnce(&Box<dyn Contract<C>>, Deps, Env) -> Result<T, String>,
    {
        let contract = self
            .contracts
            .get(&address)
            .ok_or_else(|| "Unregistered contract address".to_string())?;
        let handler = self
            .codes
            .get(&contract.code_id)
            .ok_or_else(|| "Unregistered code id".to_string())?;
        let env = self.get_env(address);

        let deps = Deps {
            storage: contract.storage.as_ref(),
            api: self.api.deref(),
            querier: QuerierWrapper::new(querier),
        };
        action(handler, deps, env)
    }
}

trait ContractProvider {
    fn get_contract<'a>(&'a self, addr: &Addr) -> Option<ContractRef<'a>>;
    fn num_contracts(&self) -> usize;
}

impl<C> ContractProvider for WasmRouter<C>
where
    C: Clone + fmt::Debug + PartialEq + JsonSchema,
{
    // Option<&'a ContractData> {
    fn get_contract<'a>(&'a self, addr: &Addr) -> Option<ContractRef<'a>> {
        self.contracts.get(addr).map(|x| x.as_ref())
    }

    fn num_contracts(&self) -> usize {
        self.contracts.len()
    }
}

/// A writable transactional cache over the wasm state.
///
/// Reads hit local hashmap or then hit router
/// Getting storage wraps the internal contract storage
///  - adding handler
///  - adding contract
///  - writing existing contract
/// Return op-log to flush, like transactional:
///  - consume this struct (release router) and return list of ops to perform
///  - pass ops &mut WasmRouter to update them
///
/// In Router, we use this exclusively in all the calls in execute (not self.wasm)
/// In Querier, we use self.wasm
pub struct WasmCache<'a, C>
where
    C: Clone + fmt::Debug + PartialEq + JsonSchema,
{
    // and this into one with reference
    router: &'a WasmRouter<C>,
    parent_contracts: &'a dyn ContractProvider,
    state: WasmCacheState<'a>,
}

impl<'a, C> ContractProvider for WasmCache<'a, C>
where
    C: Clone + fmt::Debug + PartialEq + JsonSchema,
{
    fn get_contract<'b>(&'b self, addr: &Addr) -> Option<ContractRef<'b>> {
        self.state.get_contract_ref(self.parent_contracts, addr)
    }

    fn num_contracts(&self) -> usize {
        self.parent_contracts.num_contracts() + self.state.contracts.len()
    }
}

/// This is the mutable state of the cached.
/// Separated out so we can grab a mutable reference to both these HashMaps,
/// while still getting an immutable reference to router.
/// (We cannot take &mut WasmCache)
pub struct WasmCacheState<'a> {
    contracts: HashMap<Addr, ContractData>,
    contract_diffs: HashMap<Addr, StorageTransaction<'a>>,
}

impl<'a> WasmCacheState<'a> {
    pub fn new() -> Self {
        WasmCacheState {
            contracts: HashMap::new(),
            contract_diffs: HashMap::new(),
        }
    }
}

/// This is a set of data from the WasmCache with no external reference,
/// which can be used to commit to the underlying WasmRouter.
pub struct WasmOps {
    new_contracts: HashMap<Addr, ContractData>,
    contract_diffs: Vec<(Addr, RepLog)>,
}

impl WasmOps {
    pub fn commit(self, committable: &mut dyn WasmCommittable) {
        committable.apply_ops(self)
    }
}

pub trait WasmCommittable {
    fn apply_ops(&mut self, ops: WasmOps);
}

impl<C> WasmCommittable for WasmRouter<C>
where
    C: Clone + fmt::Debug + PartialEq + JsonSchema,
{
    fn apply_ops(&mut self, ops: WasmOps) {
        ops.new_contracts.into_iter().for_each(|(k, v)| {
            self.contracts.insert(k, v);
        });
        ops.contract_diffs.into_iter().for_each(|(k, ops)| {
            let storage = self.contracts.get_mut(&k).unwrap().storage.as_mut();
            ops.commit(storage);
        });
    }
}

impl<'a, C> WasmCommittable for WasmCache<'a, C>
where
    C: Clone + fmt::Debug + PartialEq + JsonSchema,
{
    fn apply_ops(&mut self, ops: WasmOps) {
        ops.new_contracts.into_iter().for_each(|(k, v)| {
            self.state.contracts.insert(k, v);
        });
        ops.contract_diffs.into_iter().for_each(|(k, ops)| {
            match self.state.get_contract_mut(self.parent_contracts, &k) {
                Some(contract) => ops.commit(contract.storage),
                None => panic!("No contract at {}, but applying diff", k),
            }
        });
    }
}

impl<'a, C> WasmCache<'a, C>
where
    C: Clone + fmt::Debug + PartialEq + JsonSchema,
{
    fn new(router: &'a WasmRouter<C>) -> Self {
        WasmCache {
            router,
            parent_contracts: router,
            state: WasmCacheState::new(),
        }
    }

    pub fn cache(&self) -> WasmCache<C> {
        WasmCache {
            router: self.router,
            parent_contracts: self,
            state: WasmCacheState::new(),
        }
    }

    /// When we want to commit the WasmCache, we need a 2 step process to satisfy Rust reference counting:
    /// 1. prepare() consumes WasmCache, releasing &WasmRouter, and creating a self-owned update info.
    /// 2. WasmOps::commit() can now take &mut WasmRouter and updates the underlying state
    pub fn prepare(self) -> WasmOps {
        self.state.prepare()
    }

    /// This just creates an address and empty storage instance, returning the new address
    /// You must call init after this to set up the contract properly.
    /// These are separated into two steps to have cleaner return values.
    pub fn register_contract(&mut self, code_id: usize) -> Result<Addr, String> {
        if !self.router.codes.contains_key(&code_id) {
            return Err("Cannot init contract with unregistered code id".to_string());
        }
        let addr = self.next_address();
        let info = ContractData::new(code_id, (self.router.storage_factory)());
        self.state.contracts.insert(addr.clone(), info);
        Ok(addr)
    }

    // TODO: better addr generation
    fn next_address(&self) -> Addr {
        let count = self.parent_contracts.num_contracts() + self.state.contracts.len();
        // we make this longer so it is not rejected by tests
        Addr::unchecked("Contract #".to_string() + &count.to_string())
    }

    pub fn execute(
        &mut self,
        address: Addr,
        querier: &dyn Querier,
        info: MessageInfo,
        msg: Vec<u8>,
    ) -> Result<Response<C>, String> {
        let parent = &self.router.codes;
        let env = self.router.get_env(address.clone());
        let api = self.router.api.as_ref();

        self.state.with_storage(
            querier,
            self.parent_contracts,
            address,
            env,
            api,
            |code_id, deps, env| {
                let handler = parent
                    .get(&code_id)
                    .ok_or_else(|| "Unregistered code id".to_string())?;
                handler.execute(deps, env, info, msg)
            },
        )
    }

    pub fn instantiate(
        &mut self,
        address: Addr,
        querier: &dyn Querier,
        info: MessageInfo,
        msg: Vec<u8>,
    ) -> Result<Response<C>, String> {
        let parent = &self.router.codes;
        let env = self.router.get_env(address.clone());
        let api = self.router.api.as_ref();

        self.state.with_storage(
            querier,
            self.parent_contracts,
            address,
            env,
            api,
            |code_id, deps, env| {
                let handler = parent
                    .get(&code_id)
                    .ok_or_else(|| "Unregistered code id".to_string())?;
                handler.instantiate(deps, env, info, msg)
            },
        )
    }

    pub fn reply(
        &mut self,
        address: Addr,
        querier: &dyn Querier,
        reply: Reply,
    ) -> Result<Response<C>, String> {
        // this errors if the sender is not a contract
        let parent = &self.router.codes;
        let env = self.router.get_env(address.clone());
        let api = self.router.api.as_ref();

        self.state.with_storage(
            querier,
            self.parent_contracts,
            address,
            env,
            api,
            |code_id, deps, env| {
                let handler = parent
                    .get(&code_id)
                    .ok_or_else(|| "Unregistered code id".to_string())?;
                handler.reply(deps, env, reply)
            },
        )
    }

    pub fn sudo(
        &mut self,
        address: Addr,
        querier: &dyn Querier,
        msg: Vec<u8>,
    ) -> Result<Response<C>, String> {
        let parent = &self.router.codes;
        let env = self.router.get_env(address.clone());
        let api = self.router.api.as_ref();

        self.state.with_storage(
            querier,
            self.parent_contracts,
            address,
            env,
            api,
            |code_id, deps, env| {
                let handler = parent
                    .get(&code_id)
                    .ok_or_else(|| "Unregistered code id".to_string())?;
                handler.sudo(deps, env, msg)
            },
        )
    }
}

impl<'a> WasmCacheState<'a> {
    pub fn prepare(self) -> WasmOps {
        let diffs: Vec<_> = self
            .contract_diffs
            .into_iter()
            .map(|(k, store)| (k, store.prepare()))
            .collect();

        WasmOps {
            new_contracts: self.contracts,
            contract_diffs: diffs,
        }
    }

    fn get_contract_mut<'b>(
        &'b mut self,
        parent: &'a dyn ContractProvider,
        addr: &Addr,
    ) -> Option<ContractMut<'b>> {
        // if we created this transaction
        if let Some(x) = self.contracts.get_mut(addr) {
            return Some(x.as_mut());
        }
        if let Some(c) = parent.get_contract(addr) {
            let code_id = c.code_id;
            if self.contract_diffs.contains_key(addr) {
                let storage = self.contract_diffs.get_mut(addr).unwrap();
                return Some(ContractMut::new(code_id, storage));
            }
            // else make a new transaction
            let wrap = StorageTransaction::new(c.storage);
            self.contract_diffs.insert(addr.clone(), wrap);
            let storage = self.contract_diffs.get_mut(addr).unwrap();
            return Some(ContractMut::new(code_id, storage));
        } else {
            None
        }
    }

    fn get_contract_ref<'b>(
        &'b self,
        parent: &'a dyn ContractProvider,
        addr: &Addr,
    ) -> Option<ContractRef<'b>> {
        // if we created this transaction
        if let Some(x) = self.contracts.get(addr) {
            return Some(x.as_ref());
        }
        if let Some(c) = parent.get_contract(addr) {
            let code_id = c.code_id;
            if self.contract_diffs.contains_key(addr) {
                let storage = self.contract_diffs.get(addr).unwrap();
                return Some(ContractRef::new(code_id, storage));
            }
            Some(c)
        } else {
            None
        }
    }

    fn with_storage<F, T>(
        &mut self,
        querier: &dyn Querier,
        parent: &'a dyn ContractProvider,
        address: Addr,
        env: Env,
        api: &dyn Api,
        action: F,
    ) -> Result<T, String>
    where
        F: FnOnce(usize, DepsMut, Env) -> Result<T, String>,
    {
        let ContractMut { code_id, storage } = self
            .get_contract_mut(parent, &address)
            .ok_or_else(|| "Unregistered contract address".to_string())?;
        let deps = DepsMut {
            storage,
            api,
            querier: QuerierWrapper::new(querier),
        };
        action(code_id, deps, env)
    }
}

#[cfg(test)]
mod test {
    use super::*;

    use crate::test_helpers::{contract_error, contract_payout, PayoutInitMessage, PayoutQueryMsg};
    use cosmwasm_std::testing::{mock_env, mock_info, MockApi, MockQuerier, MockStorage};
    use cosmwasm_std::{coin, to_vec, BankMsg, BlockInfo, Coin, CosmosMsg, Empty};

    fn mock_router() -> WasmRouter<Empty> {
        let env = mock_env();
        let api = Box::new(MockApi::default());
        WasmRouter::new(api, env.block, || Box::new(MockStorage::new()))
    }

    #[test]
    fn register_contract() {
        let mut router = mock_router();
        let code_id = router.store_code(contract_error());
        let mut cache = router.cache();

        // cannot register contract with unregistered codeId
        cache.register_contract(code_id + 1).unwrap_err();

        // we can register a new instance of this code
        let contract_addr = cache.register_contract(code_id).unwrap();

        // now, we call this contract and see the error message from the contract
        let querier: MockQuerier<Empty> = MockQuerier::new(&[]);
        let info = mock_info("foobar", &[]);
        let err = cache
            .instantiate(contract_addr, &querier, info, b"{}".to_vec())
            .unwrap_err();
        // StdError from contract_error auto-converted to string
        assert_eq!(err, "Generic error: Init failed");

        // and the error for calling an unregistered contract
        let info = mock_info("foobar", &[]);
        let err = cache
            .instantiate(
                Addr::unchecked("unregistered"),
                &querier,
                info,
                b"{}".to_vec(),
            )
            .unwrap_err();
        // Default error message from router when not found
        assert_eq!(err, "Unregistered contract address");

        // and flush
        cache.prepare().commit(&mut router);
    }

    #[test]
    fn update_block() {
        let mut router = mock_router();

        let BlockInfo { time, height, .. } = router.get_env(Addr::unchecked("foo")).block;
        router.update_block(next_block);
        let next = router.get_env(Addr::unchecked("foo")).block;

        assert_eq!(time.plus_seconds(5), next.time);
        assert_eq!(height + 1, next.height);
    }

    #[test]
    fn contract_send_coins() {
        let mut router = mock_router();
        let code_id = router.store_code(contract_payout());
        let mut cache = router.cache();

        let contract_addr = cache.register_contract(code_id).unwrap();

        let querier: MockQuerier<Empty> = MockQuerier::new(&[]);
        let payout = coin(100, "TGD");

        // init the contract
        let info = mock_info("foobar", &[]);
        let init_msg = to_vec(&PayoutInitMessage {
            payout: payout.clone(),
        })
        .unwrap();
        let res = cache
            .instantiate(contract_addr.clone(), &querier, info, init_msg)
            .unwrap();
        assert_eq!(0, res.messages.len());

        // execute the contract
        let info = mock_info("foobar", &[]);
        let res = cache
            .execute(contract_addr.clone(), &querier, info, b"{}".to_vec())
            .unwrap();
        assert_eq!(1, res.messages.len());
        match &res.messages[0].msg {
            CosmosMsg::Bank(BankMsg::Send { to_address, amount }) => {
                assert_eq!(to_address.as_str(), "foobar");
                assert_eq!(amount.as_slice(), &[payout.clone()]);
            }
            m => panic!("Unexpected message {:?}", m),
        }

        // and flush before query
        cache.prepare().commit(&mut router);

        // query the contract
        let query = to_vec(&PayoutQueryMsg::Payout {}).unwrap();
        let data = router.query_smart(contract_addr, &querier, query).unwrap();
        let res: PayoutInitMessage = from_slice(&data).unwrap();
        assert_eq!(res.payout, payout);
    }

    fn assert_payout(cache: &mut WasmCache<Empty>, contract_addr: &Addr, payout: &Coin) {
        let querier: MockQuerier<Empty> = MockQuerier::new(&[]);
        let info = mock_info("silly", &[]);
        let res = cache
            .execute(contract_addr.clone(), &querier, info, b"{}".to_vec())
            .unwrap();
        assert_eq!(1, res.messages.len());
        match &res.messages[0].msg {
            CosmosMsg::Bank(BankMsg::Send { to_address, amount }) => {
                assert_eq!(to_address.as_str(), "silly");
                assert_eq!(amount.as_slice(), &[payout.clone()]);
            }
            m => panic!("Unexpected message {:?}", m),
        }
    }

    fn assert_no_contract(cache: &WasmCache<Empty>, contract_addr: &Addr) {
        let contract = cache.get_contract(contract_addr);
        assert!(contract.is_none(), "{:?}", contract_addr);
    }

    #[test]
    fn multi_level_wasm_cache() {
        let mut router = mock_router();
        let code_id = router.store_code(contract_payout());
        let querier: MockQuerier<Empty> = MockQuerier::new(&[]);

        // set contract 1 and commit (on router)
        let mut cache = router.cache();
        let contract1 = cache.register_contract(code_id).unwrap();
        let payout1 = coin(100, "TGD");
        let info = mock_info("foobar", &[]);
        let init_msg = to_vec(&PayoutInitMessage {
            payout: payout1.clone(),
        })
        .unwrap();
        let _res = cache
            .instantiate(contract1.clone(), &querier, info, init_msg)
            .unwrap();
        cache.prepare().commit(&mut router);

        // create a new cache and check we can use contract 1
        let mut cache = router.cache();
        assert_payout(&mut cache, &contract1, &payout1);

        // create contract 2 and use it
        let contract2 = cache.register_contract(code_id).unwrap();
        let payout2 = coin(50, "BTC");
        let info = mock_info("foobar", &[]);
        let init_msg = to_vec(&PayoutInitMessage {
            payout: payout2.clone(),
        })
        .unwrap();
        let _res = cache
            .instantiate(contract2.clone(), &querier, info, init_msg)
            .unwrap();
        assert_payout(&mut cache, &contract2, &payout2);

        // create a level2 cache and check we can use contract 1 and contract 2
        let mut cache2 = cache.cache();
        assert_payout(&mut cache2, &contract1, &payout1);
        assert_payout(&mut cache2, &contract2, &payout2);

        // create a contract on level 2
        let contract3 = cache2.register_contract(code_id).unwrap();
        let payout3 = coin(1234, "ATOM");
        let info = mock_info("johnny", &[]);
        let init_msg = to_vec(&PayoutInitMessage {
            payout: payout3.clone(),
        })
        .unwrap();
        let _res = cache2
            .instantiate(contract3.clone(), &querier, info, init_msg)
            .unwrap();
        assert_payout(&mut cache2, &contract3, &payout3);

        // ensure first cache still doesn't see this contract
        assert_no_contract(&cache, &contract3);

        // apply second to first, all contracts present
        cache2.prepare().commit(&mut cache);
        assert_payout(&mut cache, &contract1, &payout1);
        assert_payout(&mut cache, &contract2, &payout2);
        assert_payout(&mut cache, &contract3, &payout3);

        // apply to router
        cache.prepare().commit(&mut router);

        // make new cache and see all contracts there
        let mut cache3 = router.cache();
        assert_payout(&mut cache3, &contract1, &payout1);
        assert_payout(&mut cache3, &contract2, &payout2);
        assert_payout(&mut cache3, &contract3, &payout3);
    }
}
