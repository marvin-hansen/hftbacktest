use std::collections::{hash_map::Entry, BTreeMap, HashMap};

use super::{
    ApplySnapshot,
    L2MarketDepth,
    L3MarketDepth,
    L3Order,
    MarketDepth,
    INVALID_MAX,
    INVALID_MIN,
};
use crate::{
    backtest::{data::Data, BacktestError},
    prelude::{OrderId, Side},
    types::{Event, BUY_EVENT, SELL_EVENT},
};

/// L2 Market depth implementation based on a B-Tree map.
///
/// If feed data is missing, it may result in the crossing of the best bid and ask, making it
/// impossible to restore them to the most recent values through natural refreshing.
/// Ensuring data integrity is imperative.
#[derive(Debug)]
pub struct BTreeMarketDepth {
    pub tick_size: f64,
    pub lot_size: f64,
    pub timestamp: i64,
    pub bid_depth: BTreeMap<i64, f64>,
    pub ask_depth: BTreeMap<i64, f64>,
    pub best_bid_tick: i64,
    pub best_ask_tick: i64,
    pub orders: HashMap<OrderId, L3Order>,
}

impl BTreeMarketDepth {
    /// Constructs an instance of `BTreeMarketDepth`.
    pub fn new(tick_size: f64, lot_size: f64) -> Self {
        Self {
            tick_size,
            lot_size,
            timestamp: 0,
            bid_depth: Default::default(),
            ask_depth: Default::default(),
            best_bid_tick: INVALID_MIN,
            best_ask_tick: INVALID_MAX,
            orders: Default::default(),
        }
    }

    fn add(&mut self, order: L3Order) -> Result<(), BacktestError> {
        let order = match self.orders.entry(order.order_id) {
            Entry::Occupied(_) => return Err(BacktestError::OrderIdExist),
            Entry::Vacant(entry) => entry.insert(order),
        };
        if order.side == Side::Buy {
            *self.bid_depth.entry(order.price_tick).or_insert(0.0) += order.qty;
        } else {
            *self.ask_depth.entry(order.price_tick).or_insert(0.0) += order.qty;
        }
        Ok(())
    }
}

impl L2MarketDepth for BTreeMarketDepth {
    fn update_bid_depth(
        &mut self,
        price: f64,
        qty: f64,
        timestamp: i64,
    ) -> (i64, i64, i64, f64, f64, i64) {
        let price_tick = (price / self.tick_size).round() as i64;
        let prev_best_bid_tick = *self.bid_depth.keys().last().unwrap_or(&INVALID_MIN);
        let prev_qty = *self.bid_depth.get(&prev_best_bid_tick).unwrap_or(&0.0);

        if (qty / self.lot_size).round() as i64 == 0 {
            self.bid_depth.remove(&price_tick);
        } else {
            *self.bid_depth.entry(price_tick).or_insert(qty) = qty;
        }
        self.best_bid_tick = *self.bid_depth.keys().last().unwrap_or(&INVALID_MIN);
        (
            price_tick,
            prev_best_bid_tick,
            self.best_bid_tick,
            prev_qty,
            qty,
            timestamp,
        )
    }

    fn update_ask_depth(
        &mut self,
        price: f64,
        qty: f64,
        timestamp: i64,
    ) -> (i64, i64, i64, f64, f64, i64) {
        let price_tick = (price / self.tick_size).round() as i64;
        let prev_best_ask_tick = *self.bid_depth.keys().next().unwrap_or(&INVALID_MAX);
        let prev_qty = *self.ask_depth.get(&prev_best_ask_tick).unwrap_or(&0.0);

        if (qty / self.lot_size).round() as i64 == 0 {
            self.ask_depth.remove(&price_tick);
        } else {
            *self.ask_depth.entry(price_tick).or_insert(qty) = qty;
        }
        self.best_ask_tick = *self.ask_depth.keys().next().unwrap_or(&INVALID_MAX);
        (
            price_tick,
            prev_best_ask_tick,
            self.best_ask_tick,
            prev_qty,
            qty,
            timestamp,
        )
    }

    fn clear_depth(&mut self, side: Side, clear_upto_price: f64) {
        let clear_upto = (clear_upto_price / self.tick_size).round() as i64;
        if side == Side::Buy {
            let best_bid_tick = self.best_bid_tick();
            if best_bid_tick != INVALID_MIN {
                for t in clear_upto..(best_bid_tick + 1) {
                    if self.bid_depth.contains_key(&t) {
                        self.bid_depth.remove(&t);
                    }
                }
            }
            self.best_bid_tick = *self.bid_depth.keys().last().unwrap_or(&INVALID_MIN);
        } else if side == Side::Sell {
            let best_ask_tick = self.best_ask_tick();
            if best_ask_tick != INVALID_MAX {
                for t in best_ask_tick..(clear_upto + 1) {
                    if self.ask_depth.contains_key(&t) {
                        self.ask_depth.remove(&t);
                    }
                }
            }
            self.best_ask_tick = *self.ask_depth.keys().next().unwrap_or(&INVALID_MAX);
        } else {
            self.bid_depth.clear();
            self.ask_depth.clear();
        }
    }
}

impl MarketDepth for BTreeMarketDepth {
    #[inline(always)]
    fn best_bid(&self) -> f64 {
        if self.best_bid_tick == INVALID_MIN {
            f64::NAN
        } else {
            self.best_bid_tick as f64 * self.tick_size
        }
    }

    #[inline(always)]
    fn best_ask(&self) -> f64 {
        if self.best_ask_tick == INVALID_MAX {
            f64::NAN
        } else {
            self.best_ask_tick as f64 * self.tick_size
        }
    }

    #[inline(always)]
    fn best_bid_tick(&self) -> i64 {
        self.best_bid_tick
    }

    #[inline(always)]
    fn best_ask_tick(&self) -> i64 {
        self.best_ask_tick
    }

    #[inline(always)]
    fn tick_size(&self) -> f64 {
        self.tick_size
    }

    #[inline(always)]
    fn lot_size(&self) -> f64 {
        self.lot_size
    }

    #[inline(always)]
    fn bid_qty_at_tick(&self, price_tick: i64) -> f64 {
        *self.bid_depth.get(&price_tick).unwrap_or(&0.0)
    }

    #[inline(always)]
    fn ask_qty_at_tick(&self, price_tick: i64) -> f64 {
        *self.ask_depth.get(&price_tick).unwrap_or(&0.0)
    }
}

impl ApplySnapshot<Event> for BTreeMarketDepth {
    fn apply_snapshot(&mut self, data: &Data<Event>) {
        self.bid_depth.clear();
        self.ask_depth.clear();
        for row_num in 0..data.len() {
            let price = data[row_num].px;
            let qty = data[row_num].qty;

            let price_tick = (price / self.tick_size).round() as i64;
            if data[row_num].ev & BUY_EVENT == BUY_EVENT {
                *self.bid_depth.entry(price_tick).or_insert(0f64) = qty;
            } else if data[row_num].ev & SELL_EVENT == SELL_EVENT {
                *self.ask_depth.entry(price_tick).or_insert(0f64) = qty;
            }
        }
        self.best_bid_tick = *self.bid_depth.keys().last().unwrap_or(&INVALID_MIN);
        self.best_ask_tick = *self.ask_depth.keys().next().unwrap_or(&INVALID_MAX);
    }

    fn snapshot(&self) -> Vec<Event> {
        let mut events = Vec::new();

        // for (&px_tick, &qty) in self.bid_depth.iter().rev() {
        //     events.push(Event {
        //         ev: EXCH_EVENT | LOCAL_EVENT | BUY | DEPTH_SNAPSHOT_EVENT,
        //         // todo: it's not a problem now, but it would be better to have valid timestamps.
        //         exch_ts: 0,
        //         local_ts: 0,
        //         px: px_tick as f64 * self.tick_size,
        //         qty,
        //     });
        // }
        //
        // for (&px_tick, &qty) in self.ask_depth.iter() {
        //     events.push(Event {
        //         ev: EXCH_EVENT | LOCAL_EVENT | SELL | DEPTH_SNAPSHOT_EVENT,
        //         // todo: it's not a problem now, but it would be better to have valid timestamps.
        //         exch_ts: 0,
        //         local_ts: 0,
        //         px: px_tick as f64 * self.tick_size,
        //         qty,
        //     });
        // }

        events
    }
}

impl L3MarketDepth for BTreeMarketDepth {
    type Error = BacktestError;

    fn add_buy_order(
        &mut self,
        order_id: OrderId,
        px: f64,
        qty: f64,
        timestamp: i64,
    ) -> Result<(i64, i64), Self::Error> {
        let price_tick = (px / self.tick_size).round() as i64;
        self.add(L3Order {
            order_id,
            side: Side::Buy,
            price_tick,
            qty,
            timestamp,
        })?;
        let prev_best_tick = self.best_bid_tick;
        if price_tick > self.best_bid_tick {
            self.best_bid_tick = *self.bid_depth.keys().last().unwrap_or(&INVALID_MIN);
        }
        Ok((prev_best_tick, self.best_bid_tick))
    }

    fn add_sell_order(
        &mut self,
        order_id: OrderId,
        px: f64,
        qty: f64,
        timestamp: i64,
    ) -> Result<(i64, i64), Self::Error> {
        let price_tick = (px / self.tick_size).round() as i64;
        self.add(L3Order {
            order_id,
            side: Side::Sell,
            price_tick,
            qty,
            timestamp,
        })?;
        let prev_best_tick = self.best_ask_tick;
        if price_tick < self.best_ask_tick {
            self.best_ask_tick = *self.ask_depth.keys().next().unwrap_or(&INVALID_MAX);
        }
        Ok((prev_best_tick, self.best_ask_tick))
    }

    fn delete_order(
        &mut self,
        order_id: OrderId,
        _timestamp: i64,
    ) -> Result<(Side, i64, i64), Self::Error> {
        let order = self
            .orders
            .remove(&order_id)
            .ok_or(BacktestError::OrderNotFound)?;
        if order.side == Side::Buy {
            let prev_best_tick = self.best_bid_tick;

            let depth_qty = self.bid_depth.get_mut(&order.price_tick).unwrap();
            *depth_qty -= order.qty;
            if (*depth_qty / self.lot_size).round() as i64 == 0 {
                self.bid_depth.remove(&order.price_tick).unwrap();
                if order.price_tick == self.best_bid_tick {
                    self.best_bid_tick = *self.bid_depth.keys().next().unwrap_or(&INVALID_MIN);
                }
            }
            Ok((Side::Buy, prev_best_tick, self.best_bid_tick))
        } else {
            let prev_best_tick = self.best_ask_tick;

            let depth_qty = self.ask_depth.get_mut(&order.price_tick).unwrap();
            *depth_qty -= order.qty;
            if (*depth_qty / self.lot_size).round() as i64 == 0 {
                self.ask_depth.remove(&order.price_tick).unwrap();
                if order.price_tick == self.best_ask_tick {
                    self.best_ask_tick = *self.ask_depth.keys().next().unwrap_or(&INVALID_MAX);
                }
            }
            Ok((Side::Sell, prev_best_tick, self.best_ask_tick))
        }
    }

    fn modify_order(
        &mut self,
        order_id: OrderId,
        px: f64,
        qty: f64,
        timestamp: i64,
    ) -> Result<(Side, i64, i64), Self::Error> {
        let order = self
            .orders
            .get_mut(&order_id)
            .ok_or(BacktestError::OrderNotFound)?;
        if order.side == Side::Buy {
            let prev_best_tick = self.best_bid_tick;
            let price_tick = (px / self.tick_size).round() as i64;
            if price_tick != order.price_tick {
                let depth_qty = self.bid_depth.get_mut(&order.price_tick).unwrap();
                *depth_qty -= order.qty;
                if (*depth_qty / self.lot_size).round() as i64 == 0 {
                    self.bid_depth.remove(&order.price_tick).unwrap();
                    if order.price_tick == self.best_bid_tick {
                        self.best_bid_tick = *self.bid_depth.keys().last().unwrap_or(&INVALID_MIN);
                    }
                }

                order.price_tick = price_tick;
                order.qty = qty;
                order.timestamp = timestamp;

                *self.bid_depth.entry(order.price_tick).or_insert(0.0) += order.qty;

                if price_tick > self.best_bid_tick {
                    self.best_bid_tick = *self.bid_depth.keys().last().unwrap_or(&INVALID_MIN);
                }
                Ok((Side::Buy, prev_best_tick, self.best_bid_tick))
            } else {
                let depth_qty = self.bid_depth.get_mut(&order.price_tick).unwrap();
                *depth_qty += qty - order.qty;
                order.qty = qty;
                Ok((Side::Buy, self.best_bid_tick, self.best_bid_tick))
            }
        } else {
            let prev_best_tick = self.best_ask_tick;
            let price_tick = (px / self.tick_size).round() as i64;
            if price_tick != order.price_tick {
                let depth_qty = self.ask_depth.get_mut(&order.price_tick).unwrap();
                *depth_qty -= order.qty;
                if (*depth_qty / self.lot_size).round() as i64 == 0 {
                    self.ask_depth.remove(&order.price_tick).unwrap();
                    if order.price_tick == self.best_ask_tick {
                        self.best_ask_tick = *self.ask_depth.keys().next().unwrap_or(&INVALID_MAX);
                    }
                }

                order.price_tick = price_tick;
                order.qty = qty;
                order.timestamp = timestamp;

                *self.ask_depth.entry(order.price_tick).or_insert(0.0) += order.qty;

                if price_tick < self.best_ask_tick {
                    self.best_ask_tick = *self.ask_depth.keys().next().unwrap_or(&INVALID_MAX);
                }
                Ok((Side::Sell, prev_best_tick, self.best_ask_tick))
            } else {
                let depth_qty = self.ask_depth.get_mut(&order.price_tick).unwrap();
                *depth_qty += qty - order.qty;
                order.qty = qty;
                Ok((Side::Sell, self.best_ask_tick, self.best_ask_tick))
            }
        }
    }

    fn clear_depth(&mut self, side: Side) {
        if side == Side::Buy {
            self.bid_depth.clear();
        } else if side == Side::Sell {
            self.ask_depth.clear();
        } else {
            self.bid_depth.clear();
            self.ask_depth.clear();
        }
    }

    fn orders(&self) -> &HashMap<OrderId, L3Order> {
        &self.orders
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        depth::{BTreeMarketDepth, L3MarketDepth, MarketDepth, INVALID_MAX, INVALID_MIN},
        types::Side,
    };

    macro_rules! assert_eq_qty {
        ( $a:expr, $b:expr, $lot_size:ident ) => {{
            assert_eq!(
                ($a / $lot_size).round() as i64,
                ($b / $lot_size).round() as i64
            );
        }};
    }

    #[test]
    fn test_l3_add_delete_buy_order() {
        let lot_size = 0.001;
        let mut depth = BTreeMarketDepth::new(0.1, lot_size);

        let (prev_best, best) = depth.add_buy_order(1, 500.1, 0.001, 0).unwrap();
        assert_eq!(prev_best, INVALID_MIN);
        assert_eq!(best, 5001);
        assert_eq!(depth.best_bid_tick(), 5001);
        assert_eq_qty!(depth.bid_qty_at_tick(5001), 0.001, lot_size);

        assert!(depth.add_buy_order(1, 500.2, 0.001, 0).is_err());

        let (prev_best, best) = depth.add_buy_order(2, 500.3, 0.005, 0).unwrap();
        assert_eq!(prev_best, 5001);
        assert_eq!(best, 5003);
        assert_eq!(depth.best_bid_tick(), 5003);
        assert_eq_qty!(depth.bid_qty_at_tick(5003), 0.005, lot_size);

        let (prev_best, best) = depth.add_buy_order(3, 500.1, 0.005, 0).unwrap();
        assert_eq!(prev_best, 5003);
        assert_eq!(best, 5003);
        assert_eq!(depth.best_bid_tick(), 5003);
        assert_eq_qty!(depth.bid_qty_at_tick(5001), 0.006, lot_size);

        let (prev_best, best) = depth.add_buy_order(4, 500.5, 0.005, 0).unwrap();
        assert_eq!(prev_best, 5003);
        assert_eq!(best, 5005);
        assert_eq!(depth.best_bid_tick(), 5005);
        assert_eq_qty!(depth.bid_qty_at_tick(5005), 0.005, lot_size);

        assert!(depth.delete_order(10, 0).is_err());

        let (side, prev_best, best) = depth.delete_order(2, 0).unwrap();
        assert_eq!(side, Side::Buy);
        assert_eq!(prev_best, 5005);
        assert_eq!(best, 5005);
        assert_eq!(depth.best_bid_tick(), 5005);
        assert_eq_qty!(depth.bid_qty_at_tick(5003), 0.0, lot_size);

        let (side, prev_best, best) = depth.delete_order(4, 0).unwrap();
        assert_eq!(side, Side::Buy);
        assert_eq!(prev_best, 5005);
        assert_eq!(best, 5001);
        assert_eq!(depth.best_bid_tick(), 5001);
        assert_eq_qty!(depth.bid_qty_at_tick(5005), 0.0, lot_size);

        let (side, prev_best, best) = depth.delete_order(3, 0).unwrap();
        assert_eq!(side, Side::Buy);
        assert_eq!(prev_best, 5001);
        assert_eq!(best, 5001);
        assert_eq!(depth.best_bid_tick(), 5001);
        assert_eq_qty!(depth.bid_qty_at_tick(5001), 0.001, lot_size);

        let (side, prev_best, best) = depth.delete_order(1, 0).unwrap();
        assert_eq!(side, Side::Buy);
        assert_eq!(prev_best, 5001);
        assert_eq!(best, INVALID_MIN);
        assert_eq!(depth.best_bid_tick(), INVALID_MIN);
        assert_eq_qty!(depth.bid_qty_at_tick(5001), 0.0, lot_size);
    }

    #[test]
    fn test_l3_add_delete_sell_order() {
        let lot_size = 0.001;
        let mut depth = BTreeMarketDepth::new(0.1, lot_size);

        let (prev_best, best) = depth.add_sell_order(1, 500.1, 0.001, 0).unwrap();
        assert_eq!(prev_best, INVALID_MAX);
        assert_eq!(best, 5001);
        assert_eq!(depth.best_ask_tick(), 5001);
        assert_eq_qty!(depth.ask_qty_at_tick(5001), 0.001, lot_size);

        assert!(depth.add_sell_order(1, 500.2, 0.001, 0).is_err());

        let (prev_best, best) = depth.add_sell_order(2, 499.3, 0.005, 0).unwrap();
        assert_eq!(prev_best, 5001);
        assert_eq!(best, 4993);
        assert_eq!(depth.best_ask_tick(), 4993);
        assert_eq_qty!(depth.ask_qty_at_tick(4993), 0.005, lot_size);

        let (prev_best, best) = depth.add_sell_order(3, 500.1, 0.005, 0).unwrap();
        assert_eq!(prev_best, 4993);
        assert_eq!(best, 4993);
        assert_eq!(depth.best_ask_tick(), 4993);
        assert_eq_qty!(depth.ask_qty_at_tick(5001), 0.006, lot_size);

        let (prev_best, best) = depth.add_sell_order(4, 498.5, 0.005, 0).unwrap();
        assert_eq!(prev_best, 4993);
        assert_eq!(best, 4985);
        assert_eq!(depth.best_ask_tick(), 4985);
        assert_eq_qty!(depth.ask_qty_at_tick(4985), 0.005, lot_size);

        assert!(depth.delete_order(10, 0).is_err());

        let (side, prev_best, best) = depth.delete_order(2, 0).unwrap();
        assert_eq!(side, Side::Sell);
        assert_eq!(prev_best, 4985);
        assert_eq!(best, 4985);
        assert_eq!(depth.best_ask_tick(), 4985);
        assert_eq_qty!(depth.ask_qty_at_tick(4993), 0.0, lot_size);

        let (side, prev_best, best) = depth.delete_order(4, 0).unwrap();
        assert_eq!(side, Side::Sell);
        assert_eq!(prev_best, 4985);
        assert_eq!(best, 5001);
        assert_eq!(depth.best_ask_tick(), 5001);
        assert_eq_qty!(depth.ask_qty_at_tick(4985), 0.0, lot_size);

        let (side, prev_best, best) = depth.delete_order(3, 0).unwrap();
        assert_eq!(side, Side::Sell);
        assert_eq!(prev_best, 5001);
        assert_eq!(best, 5001);
        assert_eq!(depth.best_ask_tick(), 5001);
        assert_eq_qty!(depth.ask_qty_at_tick(5001), 0.001, lot_size);

        let (side, prev_best, best) = depth.delete_order(1, 0).unwrap();
        assert_eq!(side, Side::Sell);
        assert_eq!(prev_best, 5001);
        assert_eq!(best, INVALID_MAX);
        assert_eq!(depth.best_ask_tick(), INVALID_MAX);
        assert_eq_qty!(depth.ask_qty_at_tick(5001), 0.0, lot_size);
    }

    #[test]
    fn test_l3_modify_buy_order() {
        let lot_size = 0.001;
        let mut depth = BTreeMarketDepth::new(0.1, lot_size);

        let (prev_best, best) = depth.add_buy_order(1, 500.1, 0.001, 0).unwrap();
        let (prev_best, best) = depth.add_buy_order(2, 500.3, 0.005, 0).unwrap();
        let (prev_best, best) = depth.add_buy_order(3, 500.1, 0.005, 0).unwrap();
        let (prev_best, best) = depth.add_buy_order(4, 500.5, 0.005, 0).unwrap();

        assert!(depth.modify_order(10, 500.5, 0.001, 0).is_err());

        let (side, prev_best, best) = depth.modify_order(2, 500.5, 0.001, 0).unwrap();
        assert_eq!(side, Side::Buy);
        assert_eq!(prev_best, 5005);
        assert_eq!(best, 5005);
        assert_eq!(depth.best_bid_tick(), 5005);
        assert_eq_qty!(depth.bid_qty_at_tick(5005), 0.006, lot_size);

        let (side, prev_best, best) = depth.modify_order(2, 500.7, 0.002, 0).unwrap();
        assert_eq!(side, Side::Buy);
        assert_eq!(prev_best, 5005);
        assert_eq!(best, 5007);
        assert_eq!(depth.best_bid_tick(), 5007);
        assert_eq_qty!(depth.bid_qty_at_tick(5005), 0.005, lot_size);
        assert_eq_qty!(depth.bid_qty_at_tick(5007), 0.002, lot_size);

        let (side, prev_best, best) = depth.modify_order(2, 500.6, 0.002, 0).unwrap();
        assert_eq!(side, Side::Buy);
        assert_eq!(prev_best, 5007);
        assert_eq!(best, 5006);
        assert_eq!(depth.best_bid_tick(), 5006);
        assert_eq_qty!(depth.bid_qty_at_tick(5007), 0.0, lot_size);

        let _ = depth.delete_order(4, 0).unwrap();
        let (side, prev_best, best) = depth.modify_order(2, 500.0, 0.002, 0).unwrap();
        assert_eq!(side, Side::Buy);
        assert_eq!(prev_best, 5006);
        assert_eq!(best, 5001);
        assert_eq!(depth.best_bid_tick(), 5001);
        assert_eq_qty!(depth.bid_qty_at_tick(5006), 0.0, lot_size);
        assert_eq_qty!(depth.bid_qty_at_tick(5000), 0.002, lot_size);
    }

    #[test]
    fn test_l3_modify_sell_order() {
        let lot_size = 0.001;
        let mut depth = BTreeMarketDepth::new(0.1, lot_size);

        let (prev_best, best) = depth.add_sell_order(1, 500.1, 0.001, 0).unwrap();
        let (prev_best, best) = depth.add_sell_order(2, 499.3, 0.005, 0).unwrap();
        let (prev_best, best) = depth.add_sell_order(3, 500.1, 0.005, 0).unwrap();
        let (prev_best, best) = depth.add_sell_order(4, 498.5, 0.005, 0).unwrap();

        assert!(depth.modify_order(10, 500.5, 0.001, 0).is_err());

        let (side, prev_best, best) = depth.modify_order(2, 498.5, 0.001, 0).unwrap();
        assert_eq!(side, Side::Sell);
        assert_eq!(prev_best, 4985);
        assert_eq!(best, 4985);
        assert_eq!(depth.best_ask_tick(), 4985);
        assert_eq_qty!(depth.ask_qty_at_tick(4985), 0.006, lot_size);

        let (side, prev_best, best) = depth.modify_order(2, 497.7, 0.002, 0).unwrap();
        assert_eq!(side, Side::Sell);
        assert_eq!(prev_best, 4985);
        assert_eq!(best, 4977);
        assert_eq!(depth.best_ask_tick(), 4977);
        assert_eq_qty!(depth.ask_qty_at_tick(4985), 0.005, lot_size);
        assert_eq_qty!(depth.ask_qty_at_tick(4977), 0.002, lot_size);

        let (side, prev_best, best) = depth.modify_order(2, 498.1, 0.002, 0).unwrap();
        assert_eq!(side, Side::Sell);
        assert_eq!(prev_best, 4977);
        assert_eq!(best, 4981);
        assert_eq!(depth.best_ask_tick(), 4981);
        assert_eq_qty!(depth.ask_qty_at_tick(4977), 0.0, lot_size);

        let _ = depth.delete_order(4, 0).unwrap();
        let (side, prev_best, best) = depth.modify_order(2, 500.2, 0.002, 0).unwrap();
        assert_eq!(side, Side::Sell);
        assert_eq!(prev_best, 4981);
        assert_eq!(best, 5001);
        assert_eq!(depth.best_ask_tick(), 5001);
        assert_eq_qty!(depth.ask_qty_at_tick(4981), 0.0, lot_size);
        assert_eq_qty!(depth.ask_qty_at_tick(5002), 0.002, lot_size);
    }
}
