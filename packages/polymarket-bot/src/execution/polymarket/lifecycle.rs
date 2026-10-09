//! Evidence validation around the existing durable order and settlement lifecycle.
use crate::models::{FillRecord, OrderRecord, OrderState, OrderType};
use anyhow::{ensure, Result};
use rust_decimal::Decimal;
use std::collections::HashMap;

/// Fill identity, not receipt order, determines economic effects. Venue acknowledgements alone
/// never prove a filled amount. This validator also handles a cancelled FAK with late fill evidence.
pub(crate) fn validate_fills(order: &OrderRecord, fills: &[FillRecord]) -> Result<Decimal> {
    let mut seen = HashMap::new();
    let mut size = Decimal::ZERO;
    let mut notional = Decimal::ZERO;
    for fill in fills {
        ensure!(
            fill.process_id == order.request.process_id
                && fill.order_id == order.order_id
                && fill.token_id == order.request.token_id,
            "fill ownership mismatch"
        );
        ensure!(
            fill.price > Decimal::ZERO
                && fill.price <= order.request.price
                && fill.size > Decimal::ZERO
                && fill.fee >= Decimal::ZERO,
            "fill economics exceed admission"
        );
        if let Some(prior) = seen.insert(fill.fill_id, fill) {
            ensure!(
                serde_json::to_value(prior)? == serde_json::to_value(fill)?,
                "conflicting fill identity"
            );
            continue;
        }
        notional += fill.price * fill.size;
        size = size
            .checked_add(fill.size)
            .ok_or_else(|| anyhow::anyhow!("fill size overflow"))?;
    }
    let cash = order
        .request
        .metadata
        .pointer("/polymarket_execution/quantity_semantics")
        .and_then(|v| v.as_str())
        == Some("buy_cash_budget");
    let complete = if cash {
        if size > Decimal::ZERO {
            crate::store::validate_live_cumulative_fill_economics(&order.request, size, notional)?;
        }
        (order.request.price * order.request.size - notional).abs() <= Decimal::new(1, 6)
    } else {
        ensure!(
            size <= order.request.size,
            "filled shares exceed authorized quantity"
        );
        size == order.request.size
    };
    if order.state == OrderState::Filled {
        ensure!(complete, "fill evidence incomplete");
    }
    if order.state == OrderState::Rejected {
        ensure!(size == Decimal::ZERO, "rejection contradicts fills");
    }
    if order.request.order_type == OrderType::Fok
        && matches!(
            order.state,
            OrderState::Filled | OrderState::Cancelled | OrderState::Expired
        )
    {
        ensure!(
            size == Decimal::ZERO || complete,
            "FOK partial fill requires reconciliation"
        );
    }
    Ok(size)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resolution {
    Pending,
    Executed,
    DefinitiveNonexecution,
}
/// Nonterminal and unknown states keep their reservation. Zero-fill cancellation requires a
/// reconciliation pass, since FAK trade evidence can arrive after the cancellation notification.
pub(crate) fn resolution(
    order: &OrderRecord,
    fills: &[FillRecord],
    reconciled: bool,
) -> Result<Resolution> {
    let size = validate_fills(order, fills)?;
    if matches!(
        order.state,
        OrderState::Unknown
            | OrderState::Created
            | OrderState::Submitted
            | OrderState::Acknowledged
            | OrderState::PartiallyFilled
            | OrderState::CancelRequested
    ) {
        return Ok(Resolution::Pending);
    }
    if order.state == OrderState::Rejected {
        return Ok(Resolution::DefinitiveNonexecution);
    }
    if !reconciled {
        return Ok(Resolution::Pending);
    }
    Ok(if size > Decimal::ZERO {
        Resolution::Executed
    } else {
        Resolution::DefinitiveNonexecution
    })
}
