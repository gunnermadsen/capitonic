//! One installation-scoped rotating OAuth session, atomically encrypted at rest.
use anyhow::{ensure, Context, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use chrono::{DateTime, Utc};
use ring::{
    aead::{self, LessSafeKey, UnboundKey},
    rand::{SecureRandom, SystemRandom},
};
use serde::{Deserialize, Serialize};
use std::{path::PathBuf, sync::OnceLock};
use tokio::sync::Mutex;

#[derive(Clone, Serialize, Deserialize)]
pub struct Credentials {
    pub client_id: String,
    pub ext_agent_host_id: String,
    pub subject: String,
    pub access_token: String,
    pub refresh_token: String,
    pub id_token: String,
    pub scopes: Vec<String>,
    pub expires_at: DateTime<Utc>,
}
impl Credentials {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.client_id.starts_with("oaiapp_") && !self.subject.is_empty(),
            "invalid OAuth identity"
        );
        ensure!(
            self.scopes.iter().any(|s| s == "chatgpt.tokens.use.direct"),
            "ChatGPT plan permission is missing"
        );
        ensure!(
            self.scopes.iter().any(|s| s == "offline_access")
                && self.scopes.iter().any(|s| s == "resource.invoke"),
            "OAuth runtime permissions are missing"
        );
        ensure!(
            !self.access_token.trim().is_empty() && !self.refresh_token.trim().is_empty(),
            "OAuth credentials are incomplete"
        );
        ensure!(
            self.access_token == self.access_token.trim()
                && !self.access_token.contains(['\r', '\n']),
            "invalid bearer credential"
        );
        Ok(())
    }
}

fn path() -> Result<PathBuf> {
    Ok(std::env::var("POLYMARKET_OPENAI_CREDENTIAL_PATH")
        .context("OpenAI credential path is not configured")?
        .into())
}
pub fn host_id() -> Result<String> {
    use std::io::Write;
    let target = path()?.with_extension("host-id");
    if let Ok(id) = std::fs::read_to_string(&target) {
        let id = id.trim();
        ensure!(
            id.starts_with("urn:uuid:") && uuid::Uuid::parse_str(&id[9..]).is_ok(),
            "invalid agent host identity"
        );
        return Ok(id.into());
    }
    std::fs::create_dir_all(target.parent().context("host identity has no parent")?)?;
    let id = format!("urn:uuid:{}", uuid::Uuid::new_v4());
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(target)?;
    file.write_all(id.as_bytes())?;
    file.sync_all()?;
    Ok(id)
}
fn cipher() -> Result<LessSafeKey> {
    let encoded = std::env::var("POLYMARKET_OPENAI_CREDENTIAL_KEY")
        .context("OpenAI encryption key is not configured")?;
    let key = STANDARD
        .decode(encoded.trim())
        .context("invalid OpenAI encryption key encoding")?;
    UnboundKey::new(&aead::AES_256_GCM, &key)
        .map(LessSafeKey::new)
        .map_err(|_| anyhow::anyhow!("OpenAI encryption key must have 32 bytes"))
}
pub fn load() -> Result<Credentials> {
    let bytes = std::fs::read(path()?).context("OpenAI credential storage unavailable")?;
    ensure!(bytes.len() > 12, "invalid encrypted credential storage");
    let mut ciphertext = bytes[12..].to_vec();
    let nonce: [u8; 12] = bytes[..12].try_into()?;
    let plaintext = cipher()?
        .open_in_place(
            aead::Nonce::assume_unique_for_key(nonce),
            aead::Aad::empty(),
            &mut ciphertext,
        )
        .map_err(|_| anyhow::anyhow!("OpenAI credential decryption failed"))?;
    let credentials: Credentials =
        serde_json::from_slice(plaintext).context("invalid credential record")?;
    credentials.validate()?;
    Ok(credentials)
}
pub fn save(credentials: &Credentials) -> Result<()> {
    use std::io::Write;
    credentials.validate()?;
    let target = path()?;
    let parent = target.parent().context("credential path has no parent")?;
    std::fs::create_dir_all(parent)?;
    let mut nonce = [0; 12];
    SystemRandom::new()
        .fill(&mut nonce)
        .map_err(|_| anyhow::anyhow!("secure nonce generation failed"))?;
    let mut ciphertext = serde_json::to_vec(credentials)?;
    cipher()?
        .seal_in_place_append_tag(
            aead::Nonce::assume_unique_for_key(nonce),
            aead::Aad::empty(),
            &mut ciphertext,
        )
        .map_err(|_| anyhow::anyhow!("credential encryption failed"))?;
    let temporary = target.with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&temporary)?;
    file.write_all(&nonce)?;
    file.write_all(&ciphertext)?;
    file.sync_all()?;
    std::fs::rename(&temporary, &target)?;
    std::fs::File::open(parent)?.sync_all()?;
    Ok(())
}
static SESSION: OnceLock<Mutex<Option<Credentials>>> = OnceLock::new();
pub async fn access_token(client: &reqwest::Client) -> Result<String> {
    // A cancelled market request must not abandon an in-progress rotating refresh.
    let client = client.clone();
    tokio::spawn(async move { refresh_session(&client).await })
        .await
        .context("OAuth refresh task failed")?
}
async fn refresh_session(client: &reqwest::Client) -> Result<String> {
    let mut session = SESSION.get_or_init(|| Mutex::new(None)).lock().await;
    // Reauthorization replaces the durable record; recover on the next opportunity.
    *session = Some(load()?);
    let credentials = session.as_ref().context("OAuth session unavailable")?;
    if credentials.expires_at <= Utc::now() + chrono::Duration::seconds(60) {
        let response = client
            .post("https://auth.openai.com/api/accounts/oauth/token")
            .form(&[
                ("grant_type", "refresh_token"),
                ("client_id", credentials.client_id.as_str()),
                ("refresh_token", credentials.refresh_token.as_str()),
                ("resource", "https://api.openai.com/v1"),
            ])
            .send()
            .await
            .map_err(|_| anyhow::anyhow!("OAuth refresh transport unavailable"))?;
        ensure!(
            response.status().is_success(),
            "OAuth refresh denied: HTTP {}",
            response.status().as_u16()
        );
        let value: serde_json::Value = response
            .json()
            .await
            .map_err(|_| anyhow::anyhow!("invalid OAuth refresh response"))?;
        let mut updated = credentials.clone();
        updated.access_token = value["access_token"]
            .as_str()
            .context("refresh has no access token")?
            .into();
        updated.refresh_token = value["refresh_token"]
            .as_str()
            .context("refresh has no replacement token")?
            .into();
        updated.expires_at = Utc::now()
            + chrono::Duration::seconds(
                value["expires_in"]
                    .as_i64()
                    .filter(|s| *s > 0)
                    .context("refresh has no expiry")?,
            );
        if let Some(scope) = value["scope"].as_str() {
            updated.scopes = scope.split_whitespace().map(str::to_owned).collect();
        }
        if let Some(token) = value["id_token"].as_str() {
            updated.id_token = token.into();
        }
        save(&updated)?;
        *session = Some(updated);
    }
    Ok(session
        .as_ref()
        .context("OAuth session unavailable")?
        .access_token
        .clone())
}
