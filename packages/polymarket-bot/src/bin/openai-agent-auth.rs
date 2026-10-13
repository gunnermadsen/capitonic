//! Interactive installation bootstrap; runtime refresh uses the same encrypted record.
use anyhow::{ensure, Context, Result};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use chrono::Utc;
use polymarket_bot::btc::unified_model_runtime::agent::{
    auth::{self, Credentials},
    profile,
};
use serde_json::Value;
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[tokio::main]
async fn main() -> Result<()> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()?;
    if std::env::args().any(|arg| arg == "--check") {
        // The running bot is the sole rotating-token writer; diagnostics are read-only.
        let access = auth::load()?.access_token;
        verify_model(&client, &access).await?;
        println!("OpenAI subscription credentials and selected model are healthy");
        return Ok(());
    }
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let redirect = format!(
        "http://127.0.0.1:{}/auth/callback",
        listener.local_addr()?.port()
    );
    let saved = auth::load().ok();
    let host_id = saved
        .as_ref()
        .map(|c| c.ext_agent_host_id.clone())
        .map(Ok)
        .unwrap_or_else(auth::host_id)?;
    let state = uuid::Uuid::new_v4().to_string();
    let nonce = uuid::Uuid::new_v4().to_string();
    let verifier = format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    );
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    let mut url = reqwest::Url::parse("https://auth.openai.com/api/accounts/authorize")?;
    url.query_pairs_mut().extend_pairs([
        (
            "client_id",
            saved
                .as_ref()
                .map_or("dynamic_agent_client", |c| c.client_id.as_str()),
        ),
        ("ext_agent_host_id", host_id.as_str()),
        ("response_type", "code"),
        ("redirect_uri", redirect.as_str()),
        (
            "scope",
            "openid profile email offline_access resource.invoke chatgpt.tokens.use.direct",
        ),
        ("resource", "https://api.openai.com/v1"),
        ("state", state.as_str()),
        ("nonce", nonce.as_str()),
        ("code_challenge", challenge.as_str()),
        ("code_challenge_method", "S256"),
    ]);
    if saved.is_none() {
        url.query_pairs_mut()
            .append_pair("agent_name_hint", "Capitonic");
    }
    println!("Approve this installation in your browser:\n{url}");
    let (mut socket, _) =
        tokio::time::timeout(std::time::Duration::from_secs(600), listener.accept()).await??;
    let mut bytes = vec![0; 8192];
    let size =
        tokio::time::timeout(std::time::Duration::from_secs(5), socket.read(&mut bytes)).await??;
    let request = std::str::from_utf8(&bytes[..size])?;
    let path = request
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .context("invalid OAuth callback")?;
    let callback = reqwest::Url::parse(&format!("http://127.0.0.1{path}"))?;
    ensure!(callback.path() == "/auth/callback", "invalid callback path");
    let params: std::collections::HashMap<_, _> = callback.query_pairs().into_owned().collect();
    ensure!(params.get("state") == Some(&state), "OAuth state mismatch");
    let code = params
        .get("code")
        .context("authorization was not granted")?;
    let client_id = params
        .get("client_id")
        .or_else(|| saved.as_ref().map(|c| &c.client_id))
        .context("dynamic client identity missing")?;
    ensure!(
        client_id.starts_with("oaiapp_"),
        "invalid dynamic client identity"
    );
    ensure!(
        saved.as_ref().is_none_or(|c| &c.client_id == client_id),
        "reauthorization client mismatch"
    );
    let response = client
        .post("https://auth.openai.com/api/accounts/oauth/token")
        .form(&[
            ("grant_type", "authorization_code"),
            ("client_id", client_id.as_str()),
            ("code", code.as_str()),
            ("code_verifier", verifier.as_str()),
            ("redirect_uri", redirect.as_str()),
            ("resource", "https://api.openai.com/v1"),
        ])
        .send()
        .await?;
    ensure!(
        response.status().is_success(),
        "OAuth exchange failed (HTTP {})",
        response.status()
    );
    let token: Value = response.json().await?;
    let id_token = token["id_token"]
        .as_str()
        .context("missing identity token")?;
    let discovery: Value = client
        .get("https://auth.openai.com/.well-known/openid-configuration")
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let jwks_uri = discovery["jwks_uri"]
        .as_str()
        .context("identity keys unavailable")?;
    ensure!(
        reqwest::Url::parse(jwks_uri)?.host_str() == Some("auth.openai.com"),
        "untrusted identity key host"
    );
    let jwks: Value = client
        .get(jwks_uri)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let claims = validate_identity(id_token, &jwks, client_id, &nonce)?;
    ensure!(
        saved
            .as_ref()
            .is_none_or(|c| claims["sub"].as_str() == Some(c.subject.as_str())),
        "reauthorization account mismatch"
    );
    ensure!(
        claims["nonce"].as_str() == Some(nonce.as_str()),
        "OAuth nonce mismatch"
    );
    let credentials = Credentials {
        client_id: client_id.clone(),
        ext_agent_host_id: host_id,
        subject: claims["sub"].as_str().context("subject missing")?.into(),
        access_token: token["access_token"]
            .as_str()
            .context("access missing")?
            .into(),
        refresh_token: token["refresh_token"]
            .as_str()
            .context("refresh missing")?
            .into(),
        id_token: id_token.into(),
        scopes: token["scope"]
            .as_str()
            .context("scope missing")?
            .split_whitespace()
            .map(str::to_owned)
            .collect(),
        expires_at: Utc::now()
            + chrono::Duration::seconds(token["expires_in"].as_i64().context("expiry missing")?),
    };
    credentials.validate()?;
    verify_model(&client, &credentials.access_token).await?;
    auth::save(&credentials)?;
    socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nConnection: close\r\n\r\nCapitonic authorization saved. You can close this tab.").await?;
    println!("Validated subscription credentials saved encrypted; selected model is available");
    Ok(())
}

async fn verify_model(client: &reqwest::Client, access: &str) -> Result<()> {
    let response = client
        .get("https://api.openai.com/v1/models")
        .bearer_auth(access)
        .send()
        .await?;
    ensure!(
        response.status().is_success(),
        "model discovery failed (HTTP {})",
        response.status()
    );
    let models: Value = response.json().await?;
    ensure!(
        models["models"].as_array().is_some_and(|models| models
            .iter()
            .any(|m| m["slug"].as_str() == Some(profile().model)))
            || models["data"].as_array().is_some_and(|models| models
                .iter()
                .any(|m| m["id"].as_str() == Some(profile().model))),
        "pinned agent model is not available to this subscription"
    );
    Ok(())
}

fn validate_identity(token: &str, jwks: &Value, client_id: &str, nonce: &str) -> Result<Value> {
    let parts: Vec<_> = token.split('.').collect();
    ensure!(parts.len() == 3, "invalid identity token");
    let header: Value = serde_json::from_slice(&URL_SAFE_NO_PAD.decode(parts[0])?)?;
    ensure!(header["alg"] == "RS256", "unsupported identity signature");
    let kid = header["kid"]
        .as_str()
        .context("identity signing key missing")?;
    let key = jwks["keys"]
        .as_array()
        .context("identity keys unavailable")?
        .iter()
        .find(|k| k["kid"].as_str() == Some(kid) && k["kty"] == "RSA")
        .context("identity signing key unavailable")?;
    let modulus = URL_SAFE_NO_PAD.decode(key["n"].as_str().context("RSA modulus missing")?)?;
    let exponent = URL_SAFE_NO_PAD.decode(key["e"].as_str().context("RSA exponent missing")?)?;
    ring::signature::RsaPublicKeyComponents {
        n: modulus.as_slice(),
        e: exponent.as_slice(),
    }
    .verify(
        &ring::signature::RSA_PKCS1_2048_8192_SHA256,
        format!("{}.{}", parts[0], parts[1]).as_bytes(),
        &URL_SAFE_NO_PAD.decode(parts[2])?,
    )
    .map_err(|_| anyhow::anyhow!("identity signature verification failed"))?;
    let claims: Value = serde_json::from_slice(&URL_SAFE_NO_PAD.decode(parts[1])?)?;
    let audience = claims["aud"].as_str() == Some(client_id)
        || claims["aud"]
            .as_array()
            .is_some_and(|a| a.iter().any(|v| v.as_str() == Some(client_id)));
    ensure!(
        claims["iss"] == "https://auth.openai.com"
            && audience
            && claims["nonce"].as_str() == Some(nonce)
            && claims["exp"]
                .as_i64()
                .is_some_and(|exp| exp > Utc::now().timestamp())
            && claims["nbf"]
                .as_i64()
                .is_none_or(|nbf| nbf <= Utc::now().timestamp()),
        "identity claims mismatch"
    );
    Ok(claims)
}
