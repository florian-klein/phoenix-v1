//! Phoenix is a limit order book exchange on the Solana blockchain.
//!
//! It exposes a set of instructions to create, cancel, and fill orders.
//! Each event that modifies the state of the book is recorded in an event log which can
//! be queried from a transaction signature after each transaction is confirmed. This
//! allows clients to build their own order book and trade history.
//!
//! The program is able to atomically match orders and settle trades on chain. This
//! is because each market has a fixed set of users that are allowed to place limit
//! orders on the book. Users who swap against the book will have their funds settle
//! instantaneously, while the funds of users who place orders on the book will be
//! immediately available for withdraw post fill.
//!

#[macro_use]
pub mod program;
pub mod quantities;
// Note this mod is private and only exists for the purposes of IDL generation
mod shank_structs;
pub mod state;

use std::str::FromStr;

use crate::program::processor::*;

// You need to import Pubkey prior to using the declare_id macro
use solana_program::{program::set_return_data, pubkey::Pubkey};

pub fn id() -> Pubkey {
    Pubkey::from_str("PhoeNiXZ8ByJGLkxNfZRnkUfjvmuYqLR89jjFHGqdXY").unwrap()
}

use program::{
    assert_with_msg, event_recorder::EventRecorder, PhoenixInstruction, PhoenixLogContext,
    PhoenixMarketContext,
};
use solana_program::{
    account_info::{next_account_info, AccountInfo},
    entrypoint::ProgramResult,
    program_error::ProgramError,
};
use state::markets::MarketEvent;

/// This is a static PDA with seeds: [b"log"]
/// If the program id changes, this will also need to be updated
pub mod phoenix_log_authority {
    use std::str::FromStr;

    use solana_program::pubkey::Pubkey;

    pub fn id() -> Pubkey {
        Pubkey::from_str("7aDTsspkQNGKmrexAN7FLx9oxU3iPczSSvHNggyuqYkR").unwrap()
    }
}

#[cfg(not(feature = "no-entrypoint"))]
solana_program::entrypoint!(process_instruction);

pub fn process_instruction(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    instruction_data: &[u8],
) -> ProgramResult {
    let (tag, data) = instruction_data
        .split_first()
        .ok_or(ProgramError::InvalidInstructionData)?;

    let instruction =
        PhoenixInstruction::try_from(*tag).or(Err(ProgramError::InvalidInstructionData))?;

    // This is a special instruction that is only used for recording
    // inner instruction data from recursive CPI calls.
    //
    // Market events can be searched by querying the transaction hash and parsing
    // the inner instruction data according to a pre-defined schema.
    //
    // Only the log authority is allowed to call this instruction.
    if let PhoenixInstruction::Log = instruction {
        let authority = next_account_info(&mut accounts.iter())?;
        assert_with_msg(
            authority.is_signer,
            ProgramError::MissingRequiredSignature,
            "Log authority must sign through CPI",
        )?;
        return Ok(());
    }

    let (program_accounts, accounts) = accounts.split_at(4);
    let accounts_iter = &mut program_accounts.iter();
    let phoenix_log_context = PhoenixLogContext::load(accounts_iter)?;
    let market_context = if instruction == PhoenixInstruction::InitializeMarket {
        PhoenixMarketContext::load_init(accounts_iter)?
    } else {
        PhoenixMarketContext::load(accounts_iter)?
    };

    let mut event_recorder = EventRecorder::new(phoenix_log_context, &market_context, instruction)?;

    let mut record_event_fn = |e: MarketEvent<Pubkey>| event_recorder.add_event(e);
    let mut order_ids = Vec::new();

    match instruction {
        PhoenixInstruction::InitializeMarket => {
            initialize::process_initialize_market(program_id, &market_context, accounts, data)?
        }
        PhoenixInstruction::Swap => {
            new_order::process_swap(
                program_id,
                &market_context,
                accounts,
                data,
                &mut record_event_fn,
            )?;
        }
        PhoenixInstruction::SwapWithFreeFunds => {
            new_order::process_swap_with_free_funds(
                program_id,
                &market_context,
                accounts,
                data,
                &mut record_event_fn,
            )?;
        }
        PhoenixInstruction::PlaceLimitOrder => new_order::process_place_limit_order(
            program_id,
            &market_context,
            accounts,
            data,
            &mut record_event_fn,
            &mut order_ids,
        )?,
        PhoenixInstruction::PlaceLimitOrderWithFreeFunds => {
            new_order::process_place_limit_order_with_free_funds(
                program_id,
                &market_context,
                accounts,
                data,
                &mut record_event_fn,
                &mut order_ids,
            )?;
        }
        PhoenixInstruction::PlaceMultiplePostOnlyOrders => {
            new_order::process_place_multiple_post_only_orders(
                program_id,
                &market_context,
                accounts,
                data,
                &mut record_event_fn,
                &mut order_ids,
            )?;
        }
        PhoenixInstruction::PlaceMultiplePostOnlyOrdersWithFreeFunds => {
            new_order::process_place_multiple_post_only_orders_with_free_funds(
                program_id,
                &market_context,
                accounts,
                data,
                &mut record_event_fn,
                &mut order_ids,
            )?;
        }
        PhoenixInstruction::ReduceOrder => reduce_order::process_reduce_order(
            program_id,
            &market_context,
            accounts,
            data,
            true,
            &mut record_event_fn,
        )?,
        PhoenixInstruction::ReduceOrderWithFreeFunds => reduce_order::process_reduce_order(
            program_id,
            &market_context,
            accounts,
            data,
            false,
            &mut record_event_fn,
        )?,
        PhoenixInstruction::CancelAllOrders => cancel_multiple_orders::process_cancel_all_orders(
            program_id,
            &market_context,
            accounts,
            data,
            true,
            &mut record_event_fn,
        )?,
        PhoenixInstruction::CancelAllOrdersWithFreeFunds => {
            cancel_multiple_orders::process_cancel_all_orders(
                program_id,
                &market_context,
                accounts,
                data,
                false,
                &mut record_event_fn,
            )?
        }
        PhoenixInstruction::CancelUpTo => cancel_multiple_orders::process_cancel_up_to(
            program_id,
            &market_context,
            accounts,
            data,
            true,
            &mut record_event_fn,
        )?,
        PhoenixInstruction::CancelUpToWithFreeFunds => {
            cancel_multiple_orders::process_cancel_up_to(
                program_id,
                &market_context,
                accounts,
                data,
                false,
                &mut record_event_fn,
            )?
        }
        PhoenixInstruction::CancelMultipleOrdersById => {
            cancel_multiple_orders::process_cancel_multiple_orders_by_id(
                program_id,
                &market_context,
                accounts,
                data,
                true,
                &mut record_event_fn,
            )?
        }
        PhoenixInstruction::CancelMultipleOrdersByIdWithFreeFunds => {
            cancel_multiple_orders::process_cancel_multiple_orders_by_id(
                program_id,
                &market_context,
                accounts,
                data,
                false,
                &mut record_event_fn,
            )?
        }
        PhoenixInstruction::WithdrawFunds => {
            withdraw::process_withdraw_funds(program_id, &market_context, accounts, data)?;
        }
        PhoenixInstruction::DepositFunds => {
            deposit::process_deposit_funds(program_id, &market_context, accounts, data)?
        }
        PhoenixInstruction::ForceCancelOrders => governance::process_force_cancel_orders(
            program_id,
            &market_context,
            accounts,
            data,
            &mut record_event_fn,
        )?,
        PhoenixInstruction::EvictSeat => {
            governance::process_evict_seat(program_id, &market_context, accounts, data)?
        }
        PhoenixInstruction::ClaimAuthority => {
            governance::process_claim_authority(program_id, &market_context, data)?
        }
        PhoenixInstruction::NameSuccessor => {
            governance::process_name_successor(program_id, &market_context, data)?
        }
        PhoenixInstruction::ChangeMarketStatus => {
            governance::process_change_market_status(program_id, &market_context, accounts, data)?
        }
        PhoenixInstruction::RequestSeatAuthorized => manage_seat::process_request_seat_authorized(
            program_id,
            &market_context,
            accounts,
            data,
        )?,
        PhoenixInstruction::RequestSeat => {
            manage_seat::process_request_seat(program_id, &market_context, accounts, data)?
        }
        PhoenixInstruction::ChangeSeatStatus => {
            manage_seat::process_change_seat_status(program_id, &market_context, accounts, data)?;
        }
        PhoenixInstruction::CollectFees => fees::process_collect_fees(
            program_id,
            &market_context,
            accounts,
            data,
            &mut record_event_fn,
        )?,
        PhoenixInstruction::ChangeFeeRecipient => {
            fees::process_change_fee_recipient(program_id, &market_context, accounts, data)?
        }
        _ => unreachable!(),
    }
    event_recorder.increment_market_sequence_number_and_flush(market_context.market_info)?;
    // We set the order ids at the end of the instruction because the return data gets cleared after
    // every CPI call.
    if !order_ids.is_empty() {
        set_return_data(&borsh::to_vec(&order_ids)?);
    }
    Ok(())
}
