use super::*;

pub(super) fn parse_signature_type(value: Option<&str>) -> Result<SignatureType> {
    match value.unwrap_or("0").trim().to_ascii_lowercase().as_str() {
        "0" | "eoa" => Ok(SignatureType::Eoa),
        "1" | "proxy" => Ok(SignatureType::Proxy),
        "2" | "gnosis" | "gnosis_safe" | "gnosissafe" => Ok(SignatureType::GnosisSafe),
        "3" | "poly1271" | "poly_1271" => Ok(SignatureType::Poly1271),
        other => bail!("unsupported POLYMARKET_SIGNATURE_TYPE={other}"),
    }
}

pub(super) fn configured_submit_signer(
    config: &LiveExecutionConfig,
) -> Result<Option<Arc<PrivateKeySigner>>> {
    if !config.submit_auth_available() {
        return Ok(None);
    }
    let Some(private_key) = config.private_key.as_deref() else {
        return Ok(None);
    };
    Ok(Some(Arc::new(
        LocalSigner::from_str(private_key)
            .context("failed to parse POLYMARKET_PRIVATE_KEY")?
            .with_chain_id(Some(POLYGON)),
    )))
}

pub(super) fn configured_account_address(config: &LiveExecutionConfig) -> Result<Option<String>> {
    if config.private_key.is_none() {
        return Ok(None);
    }
    Ok(Some(
        canonical_configured_account_identity(config, "")?.account_address,
    ))
}

pub(super) fn canonical_configured_account_identity(
    config: &LiveExecutionConfig,
    account_ref: &str,
) -> Result<CanonicalLiveAccountIdentity> {
    let signature_type = parse_signature_type(config.signature_type.as_deref())?;
    let private_key = config
        .private_key
        .as_deref()
        .context("missing private key for canonical live account identity")?;
    let signer = LocalSigner::from_str(private_key)
        .context("failed to parse POLYMARKET_PRIVATE_KEY")?
        .with_chain_id(Some(POLYGON));
    let signer_address = signer.address().to_checksum(None).to_ascii_lowercase();
    let configured_funder = config
        .funder_address
        .as_deref()
        .filter(|address| !address.trim().is_empty())
        .map(|address| {
            Address::from_str(address)
                .context("failed to parse POLYMARKET_FUNDER_ADDRESS")
                .map(|address| address.to_checksum(None).to_ascii_lowercase())
        })
        .transpose()?;

    let account_address = match signature_type {
        SignatureType::Eoa => {
            if configured_funder
                .as_deref()
                .is_some_and(|funder| funder != signer_address)
            {
                bail!("EOA live identity has a configured funder different from the signer");
            }
            signer_address.clone()
        }
        SignatureType::Proxy => {
            let funder = configured_funder
                .as_deref()
                .context("proxy live identity requires POLYMARKET_FUNDER_ADDRESS")?;
            let expected = derive_proxy_wallet(signer.address(), POLYGON)
                .context("failed to derive signer proxy wallet")?
                .to_checksum(None)
                .to_ascii_lowercase();
            if funder != expected {
                bail!("configured proxy funder does not match the signer-derived proxy wallet");
            }
            funder.to_string()
        }
        SignatureType::GnosisSafe => {
            let funder = configured_funder
                .as_deref()
                .context("safe live identity requires POLYMARKET_FUNDER_ADDRESS")?;
            let expected = derive_safe_wallet(signer.address(), POLYGON)
                .context("failed to derive signer safe wallet")?
                .to_checksum(None)
                .to_ascii_lowercase();
            if funder != expected {
                bail!("configured safe funder does not match the signer-derived safe wallet");
            }
            funder.to_string()
        }
        SignatureType::Poly1271 => configured_funder
            .context("POLY_1271 live identity requires POLYMARKET_FUNDER_ADDRESS")?,
        _ => bail!("unsupported live signature type for canonical account identity"),
    };
    let fingerprint_sha256 = event_hash(&json!({
        "identity_version": "polymarket_live_account_v2",
        "chain_id": POLYGON,
        "account_ref": account_ref,
        "signature_type": signature_type as u8,
        "signer_address": &signer_address,
        "account_address": &account_address,
    }));
    Ok(CanonicalLiveAccountIdentity {
        account_address,
        signer_address,
        signature_type,
        fingerprint_sha256,
    })
}

pub(super) fn sdk_decimal(value: Decimal) -> Result<SdkDecimal> {
    value
        .to_string()
        .parse::<SdkDecimal>()
        .context("failed to convert decimal to SDK decimal")
}

pub(super) fn local_decimal(value: SdkDecimal) -> Result<Decimal> {
    value
        .to_string()
        .parse::<Decimal>()
        .context("failed to convert SDK decimal")
}

pub(super) fn sdk_side(side: OrderSide) -> SdkSide {
    match side {
        OrderSide::Buy => SdkSide::Buy,
        OrderSide::Sell => SdkSide::Sell,
    }
}

pub(super) fn local_side(side: SdkSide) -> OrderSide {
    match side {
        SdkSide::Sell => OrderSide::Sell,
        _ => OrderSide::Buy,
    }
}

pub(super) fn sdk_order_type(order_type: OrderType) -> Result<SdkOrderType> {
    match order_type {
        OrderType::Fok => Ok(SdkOrderType::FOK),
        OrderType::Gtc => Ok(SdkOrderType::GTC),
        OrderType::Gtd => bail!("live GTD orders require explicit expiration and are not enabled"),
    }
}

pub(super) fn local_order_type(order_type: SdkOrderType) -> OrderType {
    match order_type {
        SdkOrderType::GTC => OrderType::Gtc,
        SdkOrderType::GTD => OrderType::Gtd,
        _ => OrderType::Fok,
    }
}

pub(super) fn order_state_from_status(
    status: &OrderStatusType,
    size_matched: Option<Decimal>,
) -> OrderState {
    match status {
        OrderStatusType::Matched => OrderState::Filled,
        OrderStatusType::Canceled => OrderState::Cancelled,
        OrderStatusType::Live | OrderStatusType::Unmatched | OrderStatusType::Delayed => {
            if size_matched.unwrap_or(Decimal::ZERO) > Decimal::ZERO {
                OrderState::PartiallyFilled
            } else {
                OrderState::Acknowledged
            }
        }
        OrderStatusType::Unknown(_) => OrderState::Unknown,
        _ => OrderState::Unknown,
    }
}

pub(super) fn order_record_from_open_order(order: OpenOrderResponse) -> Result<OrderRecord> {
    let created_at = order.created_at;
    let price = local_decimal(order.price)?;
    let original_size = local_decimal(order.original_size)?;
    let size_matched = local_decimal(order.size_matched)?;
    let order_id = order.id.clone();
    let market_id = order.market.to_string();
    let token_id = order.asset_id.to_string();
    let side = local_side(order.side);
    let order_type = local_order_type(order.order_type.clone());
    let status = order.status.to_string();
    Ok(OrderRecord {
        order_id: order_id.clone(),
        request: OrderRequest {
            client_order_id: Uuid::new_v5(
                &Uuid::NAMESPACE_URL,
                format!("polymarket:venue-order:{order_id}").as_bytes(),
            ),
            process_id: None,
            market_id,
            token_id,
            side,
            order_type,
            price,
            size: original_size,
            metadata: json!({
                "source": "live_open_order",
                "venue_order_id": order_id,
                "venue_status": status,
                "outcome": order.outcome,
                "size_matched": size_matched
            }),
        },
        state: order_state_from_status(&order.status, Some(size_matched)),
        created_at,
        updated_at: Utc::now(),
    })
}

pub(super) fn is_rest_fill_trade_status(status: &TradeStatusType) -> Result<bool> {
    match status {
        TradeStatusType::Matched | TradeStatusType::Mined | TradeStatusType::Confirmed => Ok(true),
        TradeStatusType::Retrying | TradeStatusType::Failed => Ok(false),
        TradeStatusType::Unknown(value) => {
            bail!("Polymarket CLOB trade has unknown status {value}")
        }
        _ => bail!("Polymarket CLOB trade has an unsupported status"),
    }
}

pub(super) fn fill_record_from_trade_for_order(
    order: &OrderRecord,
    venue_order_id: &str,
    trade: &TradeResponse,
    checked_at: DateTime<Utc>,
) -> Result<FillRecord> {
    let process_id = order
        .request
        .process_id
        .context("live REST fill backfill requires process-owned order evidence")?;
    if venue_order_id.trim().is_empty() || venue_order_id.len() > 256 {
        bail!("live REST fill backfill has an invalid venue order identity");
    }
    if trade.id.trim().is_empty() || trade.id.trim() != trade.id || trade.id.len() > 256 {
        bail!("live REST fill backfill has an invalid venue trade identity");
    }
    if !is_rest_fill_trade_status(&trade.status)? {
        bail!("live REST fill backfill cannot persist a non-fill trade status");
    }
    if order.request.price <= Decimal::ZERO
        || order.request.price > Decimal::ONE
        || order.request.size <= Decimal::ZERO
    {
        bail!("live REST fill backfill has invalid local order economics");
    }

    let taker_match = trade.taker_order_id == venue_order_id;
    let maker_matches = trade
        .maker_orders
        .iter()
        .filter(|maker_order| maker_order.order_id == venue_order_id)
        .collect::<Vec<_>>();
    if usize::from(taker_match)
        .checked_add(maker_matches.len())
        .context("live REST fill role count overflow")?
        != 1
    {
        bail!(
            "live REST fill trade {} does not identify exactly one role for venue order {}",
            trade.id,
            venue_order_id
        );
    }

    let (asset_id, side, price, size, fee_rate_bps) = if taker_match {
        if !matches!(
            trade.trader_side,
            polymarket_client_sdk_v2::clob::types::TraderSide::Taker
        ) {
            bail!(
                "live REST fill trade {} conflicts with its authenticated taker role",
                trade.id
            );
        }
        (
            trade.asset_id,
            trade.side,
            trade.price,
            trade.size,
            trade.fee_rate_bps,
        )
    } else {
        if !matches!(
            trade.trader_side,
            polymarket_client_sdk_v2::clob::types::TraderSide::Maker
        ) {
            bail!(
                "live REST fill trade {} conflicts with its authenticated maker role",
                trade.id
            );
        }
        let maker_order = maker_matches[0];
        (
            maker_order.asset_id,
            maker_order.side,
            maker_order.price,
            maker_order.matched_amount,
            maker_order.fee_rate_bps,
        )
    };

    let token_id = asset_id.to_string();
    if token_id != order.request.token_id {
        bail!(
            "live REST fill trade {} token identity does not match local order {}",
            trade.id,
            order.order_id
        );
    }
    let side = match side {
        SdkSide::Buy => OrderSide::Buy,
        SdkSide::Sell => OrderSide::Sell,
        _ => bail!("live REST fill trade {} has an unknown side", trade.id),
    };
    if side != order.request.side {
        bail!(
            "live REST fill trade {} side does not match local order {}",
            trade.id,
            order.order_id
        );
    }

    let price = local_decimal(price)?;
    let size = local_decimal(size)?;
    let fee_rate_bps = local_decimal(fee_rate_bps)?;
    if price <= Decimal::ZERO || price > Decimal::ONE {
        bail!("live REST fill trade {} has an invalid price", trade.id);
    }
    if size <= Decimal::ZERO || (order.request.side == OrderSide::Sell && size > order.request.size)
    {
        bail!("live REST fill trade {} has an invalid size", trade.id);
    }
    if fee_rate_bps < Decimal::ZERO || fee_rate_bps > Decimal::from(10_000) {
        bail!("live REST fill trade {} has an invalid fee rate", trade.id);
    }
    let marketable = match order.request.side {
        OrderSide::Buy => price <= order.request.price,
        OrderSide::Sell => price >= order.request.price,
    };
    if !marketable {
        bail!(
            "live REST fill trade {} violates local order limit price",
            trade.id
        );
    }
    let earliest_match_time = order
        .created_at
        .checked_sub_signed(LIVE_FILL_RECONCILIATION_SKEW)
        .context("live REST fill order window underflow")?;
    let latest_match_time = order
        .created_at
        .checked_add_signed(LIVE_FILL_RECONCILIATION_SKEW)
        .context("live REST fill order window overflow")?;
    let latest_observed_match_time = checked_at
        .checked_add_signed(LIVE_FILL_RECONCILIATION_SKEW)
        .context("live REST fill observation window overflow")?;
    if trade.match_time > latest_observed_match_time
        || trade.match_time < earliest_match_time
        || trade.match_time > latest_match_time
    {
        bail!(
            "live REST fill trade {} is outside the bounded order execution window",
            trade.id
        );
    }

    let fee = live_fill_fee(order, price, size)
        .context("live REST fill is missing sealed dynamic fee evidence")?;
    Ok(FillRecord {
        fill_id: Uuid::new_v5(
            &Uuid::NAMESPACE_URL,
            format!("polymarket:trade-order:{}:{venue_order_id}", trade.id).as_bytes(),
        ),
        process_id: Some(process_id),
        order_id: order.order_id.clone(),
        token_id,
        price,
        size,
        fee,
        source: FillSource::Live,
        filled_at: trade.match_time,
    })
}
