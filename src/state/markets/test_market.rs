use std::collections::VecDeque;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::quantities::*;
use crate::state::markets::*;
use crate::state::*;
use sokoban::node_allocator::NodeAllocatorMap;
use sokoban::ZeroCopy;

use crate::state::markets::MarketEvent;

const BOOK_SIZE: usize = 4096;

type TraderId = u128;
type Dex = FIFOMarket<TraderId, BOOK_SIZE, BOOK_SIZE, 8193>;

fn setup_market() -> Dex {
    setup_market_with_params(10000, 100, 0)
}

fn setup_market_with_params(
    tick_size_in_quote_lots_per_base_unit: u64,
    base_lots_per_base_unit: u64,
    fees: u64,
) -> Dex {
    let mut data = vec![0; std::mem::size_of::<Dex>()];
    let dex = Dex::load_mut_bytes(&mut data).unwrap();
    dex.initialize_with_params(
        QuoteLotsPerBaseUnitPerTick::new(tick_size_in_quote_lots_per_base_unit),
        BaseLotsPerBaseUnit::new(base_lots_per_base_unit),
    );
    dex.set_fee(fees);
    *dex
}

/// Dummy placeholder clock function
fn get_clock_fn() -> (u64, u64) {
    (0, 0)
}

#[allow(clippy::too_many_arguments)]
fn layer_orders(
    dex: &mut Dex,
    trader: TraderId,
    start_price: u64,
    end_price: u64,
    price_step: u64,
    start_size: u64,
    size_step: u64,
    side: Side,
    event_recorder: &mut dyn FnMut(MarketEvent<TraderId>),
) {
    assert!(price_step > 0 && size_step > 0);
    let mut prices = vec![];
    let mut sizes = vec![];
    match side {
        Side::Bid => {
            assert!(start_price >= end_price);
            let mut price = start_price;
            let mut size = start_size;
            while price >= end_price && price > 0 {
                prices.push(price);
                sizes.push(size);
                price -= price_step;
                size += size_step;
            }
        }
        Side::Ask => {
            assert!(start_price <= end_price);
            let mut price = start_price;
            let mut size = start_size;
            while price <= end_price {
                prices.push(price);
                sizes.push(size);
                price += price_step;
                size += size_step;
            }
        }
    }
    let adj = dex.get_base_lots_per_base_unit().as_u64();
    for (p, s) in prices.iter().zip(sizes.iter()) {
        dex.place_order(
            &trader,
            OrderPacket::new_limit_order_default(side, *p, *s * adj),
            event_recorder,
            &mut get_clock_fn,
        )
        .unwrap();
    }
}
