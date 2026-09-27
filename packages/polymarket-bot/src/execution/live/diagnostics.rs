use super::*;

#[allow(clippy::too_many_arguments)]
pub(super) async fn wallet_candidate_address_diagnostics(
    requested_candidates: Vec<String>,
    configured_funder_address: Option<&str>,
    signer_address: Option<&str>,
    authenticated_client_address: Option<&str>,
    derived_proxy_wallet_address: Option<&str>,
    derived_safe_wallet_address: Option<&str>,
    relayer_base_url: &str,
    rpc_url: &str,
) -> Vec<LiveWalletCandidateAddressDiagnostics> {
    let mut candidates = Vec::new();
    push_unique_candidate_address(&mut candidates, signer_address);
    push_unique_candidate_address(&mut candidates, configured_funder_address);
    push_unique_candidate_address(&mut candidates, authenticated_client_address);
    push_unique_candidate_address(&mut candidates, derived_proxy_wallet_address);
    push_unique_candidate_address(&mut candidates, derived_safe_wallet_address);
    for candidate in requested_candidates {
        push_unique_candidate_address(&mut candidates, Some(&candidate));
    }

    let mut diagnostics = Vec::with_capacity(candidates.len());
    for address in candidates {
        let (
            deployed_as_deposit_wallet,
            deployed_as_deposit_wallet_error,
            deposit_wallet_deployment_check_url,
        ) = check_candidate_relayer_deployment(relayer_base_url, Some(&address), "WALLET").await;
        let (
            deployed_as_safe_wallet,
            deployed_as_safe_wallet_error,
            safe_wallet_deployment_check_url,
        ) = check_candidate_relayer_deployment(relayer_base_url, Some(&address), "SAFE").await;
        let balances = Some(wallet_token_balances(rpc_url, &address).await);
        diagnostics.push(LiveWalletCandidateAddressDiagnostics {
            address: address.clone(),
            matches_signer: addresses_equal(Some(&address), signer_address),
            matches_configured_funder: addresses_equal(Some(&address), configured_funder_address),
            matches_authenticated_client: addresses_equal(
                Some(&address),
                authenticated_client_address,
            ),
            matches_proxy_wallet: addresses_equal(Some(&address), derived_proxy_wallet_address),
            matches_safe_wallet: addresses_equal(Some(&address), derived_safe_wallet_address),
            deployed_as_deposit_wallet,
            deployed_as_deposit_wallet_error,
            deposit_wallet_deployment_check_url,
            deployed_as_safe_wallet,
            deployed_as_safe_wallet_error,
            safe_wallet_deployment_check_url,
            balances,
            poly1271_authenticated_client_address: None,
            poly1271_api_keys_readable: false,
            poly1271_api_keys_error: None,
            poly1271_balance_allowance_readable: false,
            poly1271_balance_allowance_error: None,
            poly1271_collateral_balance: None,
            poly1271_open_orders_readable: false,
            poly1271_open_orders_error: None,
            poly1271_open_orders_count: None,
        });
    }
    diagnostics
}

pub(super) async fn wallet_token_balances(rpc_url: &str, address: &str) -> LiveWalletTokenBalances {
    let mut balances = LiveWalletTokenBalances {
        address: address.to_string(),
        pol_wei: None,
        pusd: None,
        usdc_e: None,
        native_usdc: None,
        error: None,
    };

    match rpc_balance_snapshot(rpc_url, address).await {
        Ok(snapshot) => {
            balances.pol_wei = Some(snapshot.pol_wei);
            balances.pusd = Some(snapshot.pusd);
            balances.usdc_e = Some(snapshot.usdc_e);
            balances.native_usdc = Some(snapshot.native_usdc);
        }
        Err(error) => balances.error = Some(error.to_string()),
    }

    balances
}

struct WalletBalanceSnapshot {
    pol_wei: String,
    pusd: String,
    usdc_e: String,
    native_usdc: String,
}

async fn rpc_balance_snapshot(rpc_url: &str, address: &str) -> Result<WalletBalanceSnapshot> {
    Ok(WalletBalanceSnapshot {
        pol_wei: rpc_call_quantity(rpc_url, "eth_getBalance", json!([address, "latest"])).await?,
        pusd: erc20_balance_of(rpc_url, PUSD_ADDRESS, address).await?,
        usdc_e: erc20_balance_of(rpc_url, USDC_E_ADDRESS, address).await?,
        native_usdc: erc20_balance_of(rpc_url, NATIVE_USDC_ADDRESS, address).await?,
    })
}

pub(super) async fn erc20_balance_of(
    rpc_url: &str,
    token_address: &str,
    owner_address: &str,
) -> Result<String> {
    let owner = owner_address
        .strip_prefix("0x")
        .unwrap_or(owner_address)
        .to_ascii_lowercase();
    let data = format!("0x70a08231{:0>64}", owner);
    rpc_call_quantity(
        rpc_url,
        "eth_call",
        json!([{"to": token_address, "data": data}, "latest"]),
    )
    .await
}

pub(super) async fn rpc_call_quantity(
    rpc_url: &str,
    method: &str,
    params: Value,
) -> Result<String> {
    let response: RpcResponse = reqwest::Client::new()
        .post(rpc_url)
        .json(&json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": method,
            "params": params
        }))
        .send()
        .await
        .with_context(|| format!("failed to request Polygon RPC method {method}"))?
        .error_for_status()
        .with_context(|| format!("Polygon RPC method {method} returned non-success status"))?
        .json()
        .await
        .with_context(|| format!("failed to decode Polygon RPC response for {method}"))?;

    if let Some(error) = response.error {
        bail!("Polygon RPC method {method} returned error: {error}");
    }

    let raw = response
        .result
        .with_context(|| format!("Polygon RPC method {method} did not return a result"))?;
    u256_hex_to_decimal_string(&raw)
}

pub(super) fn u256_hex_to_decimal_string(value: &str) -> Result<String> {
    let trimmed = value.strip_prefix("0x").unwrap_or(value);
    if trimmed.is_empty() {
        return Ok("0".to_string());
    }
    let parsed = U256::from_str_radix(trimmed, 16)
        .with_context(|| format!("failed to parse U256 hex quantity {value}"))?;
    Ok(parsed.to_string())
}
