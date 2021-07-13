#![cfg(test)]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::fmt;

use cosmwasm_std::{
    attr, to_binary, BankMsg, Binary, Coin, Deps, DepsMut, Empty, Env, MessageInfo, Reply,
    Response, StdError, SubMsg,
};
use cw_storage_plus::{Item, Map, U64Key};

use crate::wasm::{Contract, ContractWrapper};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct EmptyMsg {}

fn instantiate_error(
    _deps: DepsMut,
    _env: Env,
    _info: MessageInfo,
    _msg: EmptyMsg,
) -> Result<Response, StdError> {
    Err(StdError::generic_err("Init failed"))
}

fn execute_error(
    _deps: DepsMut,
    _env: Env,
    _info: MessageInfo,
    _msg: EmptyMsg,
) -> Result<Response, StdError> {
    Err(StdError::generic_err("Handle failed"))
}

fn query_error(_deps: Deps, _env: Env, _msg: EmptyMsg) -> Result<Binary, StdError> {
    Err(StdError::generic_err("Query failed"))
}

pub fn contract_error() -> Box<dyn Contract<Empty>> {
    let contract: ContractWrapper<_, _, _, _, _, _, _, _, _> =
        ContractWrapper::new(execute_error, instantiate_error, query_error);
    Box::new(contract)
}

#[allow(dead_code)]
pub fn contract_error_custom<C>() -> Box<dyn Contract<C>>
where
    C: Clone + fmt::Debug + PartialEq + JsonSchema + 'static,
{
    let contract: ContractWrapper<_, _, _, _, _, _, _, _, _> =
        ContractWrapper::new_with_empty(execute_error, instantiate_error, query_error);
    Box::new(contract)
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PayoutInitMessage {
    pub payout: Coin,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PayoutSudoMsg {
    pub set_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PayoutQueryMsg {
    Count {},
    Payout {},
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PayoutCountResponse {
    pub count: u32,
}

const PAYOUT: Item<PayoutInitMessage> = Item::new("payout");
const COUNT: Item<u32> = Item::new("count");

fn instantiate_payout(
    deps: DepsMut,
    _env: Env,
    _info: MessageInfo,
    msg: PayoutInitMessage,
) -> Result<Response, StdError> {
    PAYOUT.save(deps.storage, &msg)?;
    COUNT.save(deps.storage, &1)?;
    Ok(Response::default())
}

fn execute_payout(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    _msg: EmptyMsg,
) -> Result<Response, StdError> {
    // always try to payout what was set originally
    let payout = PAYOUT.load(deps.storage)?;
    let msg = SubMsg::new(BankMsg::Send {
        to_address: info.sender.into(),
        amount: vec![payout.payout],
    });
    let res = Response {
        messages: vec![msg],
        attributes: vec![attr("action", "payout")],
        events: vec![],
        data: None,
    };
    Ok(res)
}

fn sudo_payout(deps: DepsMut, _env: Env, msg: PayoutSudoMsg) -> Result<Response, StdError> {
    COUNT.save(deps.storage, &msg.set_count)?;
    Ok(Response::default())
}

fn query_payout(deps: Deps, _env: Env, msg: PayoutQueryMsg) -> Result<Binary, StdError> {
    match msg {
        PayoutQueryMsg::Count {} => {
            let count = COUNT.load(deps.storage)?;
            let res = PayoutCountResponse { count };
            to_binary(&res)
        }
        PayoutQueryMsg::Payout {} => {
            let payout = PAYOUT.load(deps.storage)?;
            to_binary(&payout)
        }
    }
}

pub fn contract_payout() -> Box<dyn Contract<Empty>> {
    let contract = ContractWrapper::new_with_sudo(
        execute_payout,
        instantiate_payout,
        query_payout,
        sudo_payout,
    );
    Box::new(contract)
}

pub fn contract_payout_custom<C>() -> Box<dyn Contract<C>>
where
    C: Clone + fmt::Debug + PartialEq + JsonSchema + 'static,
{
    let contract =
        ContractWrapper::new_with_empty(execute_payout, instantiate_payout, query_payout);
    Box::new(contract)
}

/// This is just a demo place so we can test custom message handling
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename = "snake_case")]
pub enum CustomMsg {
    SetName { name: String },
    SetAge { age: u32 },
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ReflectMessage {
    pub messages: Vec<SubMsg<CustomMsg>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ReflectQueryMsg {
    Count {},
    Reply { id: u64 },
}

const REFLECT: Map<U64Key, Reply> = Map::new("reflect");

fn instantiate_reflect(
    deps: DepsMut,
    _env: Env,
    _info: MessageInfo,
    _msg: EmptyMsg,
) -> Result<Response<CustomMsg>, StdError> {
    COUNT.save(deps.storage, &0)?;
    Ok(Response::default())
}

fn execute_reflect(
    deps: DepsMut,
    _env: Env,
    _info: MessageInfo,
    msg: ReflectMessage,
) -> Result<Response<CustomMsg>, StdError> {
    COUNT.update::<_, StdError>(deps.storage, |old| Ok(old + 1))?;

    let res = Response {
        messages: msg.messages,
        attributes: vec![],
        events: vec![],
        data: None,
    };
    Ok(res)
}

fn query_reflect(deps: Deps, _env: Env, msg: ReflectQueryMsg) -> Result<Binary, StdError> {
    match msg {
        ReflectQueryMsg::Count {} => {
            let count = COUNT.load(deps.storage)?;
            let res = PayoutCountResponse { count };
            to_binary(&res)
        }
        ReflectQueryMsg::Reply { id } => {
            let reply = REFLECT.load(deps.storage, id.into())?;
            to_binary(&reply)
        }
    }
}

fn reply_reflect(deps: DepsMut, _env: Env, msg: Reply) -> Result<Response<CustomMsg>, StdError> {
    REFLECT.save(deps.storage, msg.id.into(), &msg)?;
    Ok(Response::default())
}

pub fn contract_reflect() -> Box<dyn Contract<CustomMsg>> {
    let contract = ContractWrapper::new_with_reply(
        execute_reflect,
        instantiate_reflect,
        query_reflect,
        reply_reflect,
    );
    Box::new(contract)
}
