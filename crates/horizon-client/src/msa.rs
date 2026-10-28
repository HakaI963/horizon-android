//! Microsoft sign-in on the phone: the device-code flow, Xbox Live, XSTS, and the
//! Minecraft profile behind them.
//!
//! The desktop launcher does this in `launcher/src/account.rs`, which is a separate
//! program with a keychain and a window of its own. A phone has neither: the launcher
//! is a page inside the game's own loop, there is no keychain it may reach, and the
//! only way to show a browser is an intent. So the flow is here instead, over the same
//! endpoints and with the same steps, and the tokens go to a file in this app's
//! private storage with mode 0600 — the one place on the phone nothing else can read.
//!
//! The public half of an account — its name and UUID — goes in `accounts.txt` beside
//! the skins, where a file manager can see it. Never a token, and never a refresh
//! token anywhere but the private file.
//!
//! The Azure application is configuration, not code: `HORIZON_MSA_CLIENT_ID` at build
//! time or at run time. Until one is set, `client_id()` is `None` and the launcher says
//! so instead of starting a flow that cannot finish.

use serde_json::{json, Value};
use std::{
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

/// The scopes a Minecraft sign-in needs. `offline_access` is what gives a refresh
/// token, which is the only way the player does not sign in again tomorrow.
const SCOPE: &str = "XboxLive.signin offline_access";
const CONSUMERS: &str = "https://login.microsoftonline.com/consumers/oauth2/v2.0";

/// The Azure application's client ID, from the environment or baked in at build time.
/// Empty or unset means sign-in is not configured on this build.
pub fn client_id() -> Option<String> {
    let value = std::env::var("HORIZON_MSA_CLIENT_ID")
        .ok()
        .or_else(|| option_env!("HORIZON_MSA_CLIENT_ID").map(str::to_owned))?;
    let value = value.trim().to_owned();
    (!value.is_empty()).then_some(value)
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(20))
        .timeout_read(Duration::from_secs(30))
        .user_agent("Horizon-Android")
        .build()
}

/// What the player has to do in the browser, and the secret that finishes the flow.
pub struct DeviceCode {
    /// The short code to type at `verification_uri`.
    pub user_code: String,
    pub verification_uri: String,
    /// Seconds between polls, as Microsoft asks.
    pub interval: u64,
    pub expires_at: u64,
    device_code: String,
}

impl std::fmt::Debug for DeviceCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DeviceCode")
            .field("user_code", &self.user_code)
            .field("verification_uri", &self.verification_uri)
            .finish_non_exhaustive()
    }
}

/// The Microsoft tokens. The access token only lives long enough for the Xbox step.
pub struct MsTokens {
    access: String,
    refresh: String,
}

impl std::fmt::Debug for MsTokens {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("MsTokens(<hidden>)")
    }
}

/// One poll's answer.
pub enum Poll {
    Pending,
    SlowDown,
    Done(MsTokens),
}

/// Ask Microsoft for a code. This is the first request of the flow and the only one
/// that produces something to show the player.
pub fn device_code(client_id: &str) -> Result<DeviceCode, String> {
    let body = form(&[("client_id", client_id), ("scope", SCOPE)]);
    let (status, value) = post_form(&format!("{CONSUMERS}/devicecode"), &body, "Microsoft")?;
    match (
        status,
        text(&value, "user_code"),
        text(&value, "device_code"),
    ) {
        (200, Some(user_code), Some(device_code)) => Ok(DeviceCode {
            user_code,
            verification_uri: text(&value, "verification_uri")
                .unwrap_or_else(|| "https://www.microsoft.com/link".into()),
            interval: value
                .get("interval")
                .and_then(Value::as_u64)
                .unwrap_or(5)
                .max(1),
            expires_at: now() + value.get("expires_in").and_then(Value::as_u64).unwrap_or(900),
            device_code,
        }),
        _ => Err(oauth_error(&value)),
    }
}

/// One poll of the token endpoint.
pub fn poll(client_id: &str, code: &DeviceCode) -> Result<Poll, String> {
    let body = form(&[
        ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
        ("client_id", client_id),
        ("device_code", &code.device_code),
    ]);
    let (status, value) = post_form(&format!("{CONSUMERS}/token"), &body, "Microsoft")?;
    match value.get("error").and_then(Value::as_str) {
        Some("authorization_pending") => return Ok(Poll::Pending),
        Some("slow_down") => return Ok(Poll::SlowDown),
        _ => {}
    }
    match (
        status,
        text(&value, "access_token"),
        text(&value, "refresh_token"),
    ) {
        (200, Some(access), Some(refresh)) => Ok(Poll::Done(MsTokens { access, refresh })),
        _ => Err(oauth_error(&value)),
    }
}

/// Wait for the player to approve in the browser, polling as Microsoft asks.
///
/// `cancelled` is checked between polls, so a player who backs out of the page does
/// not leave a thread asking Microsoft for fifteen minutes. The flag is shared with
/// the caller's worker, which is how the launcher's Cancel button stops it.
pub fn wait_for_sign_in(
    client_id: &str,
    code: &DeviceCode,
    cancelled: &AtomicBool,
) -> Result<MsTokens, String> {
    let mut interval = code.interval;
    loop {
        if now() > code.expires_at {
            return Err("The sign-in code expired. Start again.".into());
        }
        let until = std::time::Instant::now() + Duration::from_secs(interval);
        while std::time::Instant::now() < until {
            if cancelled.load(Ordering::Relaxed) {
                return Err(CANCELLED.into());
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        match poll(client_id, code)? {
            Poll::Pending => {}
            Poll::SlowDown => interval += 5,
            Poll::Done(tokens) => return Ok(tokens),
        }
    }
}

/// What a cancelled sign-in says, in one place so the launcher can recognise it.
pub const CANCELLED: &str = "Sign-in cancelled.";

/// A signed-in Minecraft account: the public half, plus the tokens that let the game
/// join online-mode servers.
#[derive(Clone)]
pub struct Account {
    pub name: String,
    pub uuid: String,
    pub access_token: String,
    pub refresh_token: String,
    /// When the Minecraft token stops being usable, in Unix seconds.
    pub expires_at: u64,
}

/// Never the tokens: `Account` travels through `log::info!` and crash reports.
impl std::fmt::Debug for Account {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Account")
            .field("name", &self.name)
            .field("uuid", &self.uuid)
            .field("expires_at", &self.expires_at)
            .finish_non_exhaustive()
    }
}

/// A token with less than this left is renewed before the game uses it: a join that
/// starts a minute before the expiry would otherwise fail halfway through.
pub const RENEW_BEFORE: u64 = 3600;

/// Whether `account`'s Minecraft token has to be renewed before the game may use it.
pub fn needs_refresh(account: &Account) -> bool {
    account.expires_at <= now().saturating_add(RENEW_BEFORE)
}

/// The in-memory session the game plays as, built from a stored account. `None` when
/// the account's UUID is not one, which the session parser would refuse anyway.
pub fn session(account: &Account) -> Option<crate::auth::AuthSession> {
    let uuid = crate::auth::parse_uuid(&account.uuid)?;
    Some(crate::auth::AuthSession::new(
        &account.name,
        uuid,
        &account.access_token,
    ))
}

/// Steps 3 to 7 of the flow: Xbox Live, XSTS, the Minecraft login, ownership and the
/// profile. Everything after the browser.
pub fn finish_sign_in(ms: &MsTokens) -> Result<Account, String> {
    // Xbox Live: the Microsoft token becomes an XBL token and a user hash.
    let (status, value) = post(
        "https://user.auth.xboxlive.com/user/authenticate",
        &json!({
            "Properties": {
                "AuthMethod": "RPS",
                "SiteName": "user.auth.xboxlive.com",
                "RpsTicket": format!("d={}", ms.access),
            },
            "RelyingParty": "http://auth.xboxlive.com",
            "TokenType": "JWT",
        })
        .to_string(),
        "Xbox Live",
    )?;
    let xbl = text(&value, "Token").ok_or_else(|| format!("Xbox Live refused ({status})"))?;
    let uhs = value
        .get("DisplayClaims")
        .and_then(|d| d.get("xui"))
        .and_then(Value::as_array)
        .and_then(|list| list.first())
        .and_then(|first| first.get("uhs"))
        .and_then(Value::as_str)
        .ok_or("Xbox Live gave no user hash")?
        .to_owned();

    // XSTS, for the Minecraft relying party.
    let (status, value) = post(
        "https://xsts.auth.xboxlive.com/xsts/authorize",
        &json!({
            "Properties": { "SandboxId": "RETAIL", "UserTokens": [xbl] },
            "RelyingParty": "rp://api.minecraftservices.com/",
            "TokenType": "JWT",
        })
        .to_string(),
        "Xbox Live",
    )?;
    let xsts = text(&value, "Token").ok_or_else(|| xsts_error(&value, status))?;

    // Minecraft.
    let (status, value) = post(
        "https://api.minecraftservices.com/authentication/login_with_xbox",
        &json!({ "identityToken": format!("XBL3.0 x={uhs};{xsts}") }).to_string(),
        "Minecraft",
    )?;
    let access = text(&value, "access_token").ok_or_else(|| match status {
        403 => "This app is not approved by Mojang yet, so Minecraft sign-in cannot finish."
            .to_string(),
        _ => format!("Minecraft refused the sign-in ({status})"),
    })?;
    let expires_in = value.get("expires_in").and_then(Value::as_u64).unwrap_or(86_400);

    // Ownership, then the profile: an account without the game is not one to play on.
    let (status, owned) = get(
        "https://api.minecraftservices.com/entitlements/mcstore",
        Some(&access),
    )?;
    if status != 200 {
        return Err("Could not check which games this account owns. Try again.".into());
    }
    // mcstore answers 200 with an `items` list; an empty list means no game.
    let owns = owned
        .get("items")
        .and_then(Value::as_array)
        .is_some_and(|items| {
            items.iter().any(|item| {
                matches!(
                    item.get("name").and_then(Value::as_str),
                    Some("game_minecraft" | "product_minecraft")
                )
            })
        });
    if !owns {
        return Err("This account doesn't own Minecraft Java Edition.".into());
    }
    let (status, profile) = get(
        "https://api.minecraftservices.com/minecraft/profile",
        Some(&access),
    )?;
    if status == 404 {
        return Err("This account has no Minecraft profile yet: make one at minecraft.net.".into());
    }
    let name = text(&profile, "name").ok_or_else(|| format!("Minecraft gave no profile ({status})"))?;
    let uuid = text(&profile, "id").ok_or("Minecraft gave no profile UUID")?;
    Ok(Account {
        name,
        uuid,
        access_token: access,
        refresh_token: ms.refresh.clone(),
        expires_at: now() + expires_in.saturating_sub(60),
    })
}

/// A renewed Minecraft token from a refresh token, for the next launch.
pub fn refresh(account: &Account) -> Result<Account, String> {
    let Some(client_id) = client_id() else {
        return Err("Microsoft sign-in is not configured on this build.".into());
    };
    let body = form(&[
        ("grant_type", "refresh_token"),
        ("client_id", &client_id),
        ("refresh_token", &account.refresh_token),
        ("scope", SCOPE),
    ]);
    let (status, value) = post_form(&format!("{CONSUMERS}/token"), &body, "Microsoft")?;
    if status != 200 {
        return Err(oauth_error(&value));
    }
    let (Some(access), Some(refresh)) = (text(&value, "access_token"), text(&value, "refresh_token"))
    else {
        return Err(oauth_error(&value));
    };
    let ms = MsTokens { access, refresh };
    finish_sign_in(&ms)
}

// ----- Storage -----------------------------------------------------------------------

/// Where the tokens live: this app's private storage, never the folder a file manager
/// can read. `<internal>/horizon/accounts/` on a phone (the activity sets
/// `HORIZON_HOME` there) and `~/.local/share/horizon/accounts/` on a desktop.
fn token_dir() -> PathBuf {
    crate::logging::game_home().join("accounts")
}

/// Keep an account's tokens for the next launch. The file is mode 0600 on Unix, so no
/// other app on the phone can read it, and an existing file's mode is corrected too:
/// `mode()` alone only applies when the file is created.
pub fn store(account: &Account) -> Result<(), String> {
    let dir = token_dir();
    std::fs::create_dir_all(&dir).map_err(|error| format!("{}: {error}", dir.display()))?;
    let path = dir.join(format!("{}.json", account.uuid));
    let body = json!({
        "name": account.name,
        "uuid": account.uuid,
        "access_token": account.access_token,
        "refresh_token": account.refresh_token,
        "expires_at": account.expires_at,
    });
    let text = serde_json::to_string_pretty(&body).map_err(|error| error.to_string())?;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    use std::io::Write as _;
    let mut file = options
        .open(&path)
        .map_err(|error| format!("{}: {error}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = file.set_permissions(std::fs::Permissions::from_mode(0o600));
    }
    file.write_all(text.as_bytes())
        .map_err(|error| format!("{}: {error}", path.display()))?;
    Ok(())
}

/// Every stored account, by name so the list keeps its order between frames. A file
/// that does not parse is skipped rather than failing the list: one corrupt entry
/// must not hide the rest.
pub fn stored() -> Vec<Account> {
    let dir = token_dir();
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut accounts: Vec<Account> = entries
        .flatten()
        .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "json"))
        .filter_map(|entry| std::fs::read_to_string(entry.path()).ok())
        .filter_map(|text| serde_json::from_str::<Value>(&text).ok())
        .filter_map(|value| {
            Some(Account {
                name: text(&value, "name")?,
                uuid: text(&value, "uuid")?,
                access_token: text(&value, "access_token")?,
                refresh_token: text(&value, "refresh_token")?,
                expires_at: value.get("expires_at").and_then(Value::as_u64).unwrap_or(0),
            })
        })
        .collect();
    accounts.sort_by(|a, b| a.name.cmp(&b.name));
    accounts
}

/// The stored account with this player name, if this device has signed it in.
pub fn stored_named(name: &str) -> Option<Account> {
    stored().into_iter().find(|account| account.name == name)
}

/// Forget an account's tokens.
pub fn forget(uuid: &str) {
    let _ = std::fs::remove_file(token_dir().join(format!("{uuid}.json")));
}

/// The session the game reads at start: `{"name","uuid","access_token","user_type"}` in
/// the shape `auth::AuthSession::parse` accepts.
pub fn session_json(account: &Account) -> String {
    json!({
        "name": account.name,
        "uuid": account.uuid,
        "access_token": account.access_token,
        "user_type": "msa",
    })
    .to_string()
}

// ----- HTTP ---------------------------------------------------------------------------

fn form(pairs: &[(&str, &str)]) -> String {
    pairs
        .iter()
        .map(|(key, value)| format!("{key}={}", encode(value)))
        .collect::<Vec<_>>()
        .join("&")
}

fn encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(byte as char)
            }
            b' ' => out.push('+'),
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

fn post(url: &str, body: &str, what: &str) -> Result<(u16, Value), String> {
    let response = agent()
        .post(url)
        .set("Content-Type", "application/json")
        .send_string(body);
    read(response, what)
}

/// The OAuth endpoints take `application/x-www-form-urlencoded`, not JSON: sending a
/// form body as JSON is what made the device-code call answer 400 with no code.
fn post_form(url: &str, body: &str, what: &str) -> Result<(u16, Value), String> {
    let response = agent()
        .post(url)
        .set("Content-Type", "application/x-www-form-urlencoded")
        .send_string(body);
    read(response, what)
}

fn get(url: &str, token: Option<&str>) -> Result<(u16, Value), String> {
    let mut request = agent().get(url);
    if let Some(token) = token {
        request = request.set("Authorization", &format!("Bearer {token}"));
    }
    read(request.call(), "Minecraft")
}

fn read(
    response: Result<ureq::Response, ureq::Error>,
    what: &str,
) -> Result<(u16, Value), String> {
    match response {
        Ok(response) => {
            let status = response.status();
            let text = response.into_string().unwrap_or_default();
            Ok((status, serde_json::from_str(&text).unwrap_or(Value::Null)))
        }
        // A 4xx is an answer, not a transport failure: the body says which.
        Err(ureq::Error::Status(status, response)) => {
            let text = response.into_string().unwrap_or_default();
            Ok((status, serde_json::from_str(&text).unwrap_or(Value::Null)))
        }
        Err(error) => Err(format!("{what} is unreachable: {error}")),
    }
}

fn text(value: &Value, key: &str) -> Option<String> {
    value.get(key).and_then(Value::as_str).map(str::to_owned)
}

fn oauth_error(value: &Value) -> String {
    match value.get("error").and_then(Value::as_str) {
        Some("authorization_declined") => "Sign-in was declined in the browser.".into(),
        Some("expired_token") => "The sign-in code expired. Start again.".into(),
        Some("bad_verification_code") => "The sign-in code was not recognised. Start again.".into(),
        Some("invalid_grant") => "Your Microsoft sign-in expired. Sign in again.".into(),
        Some("invalid_client" | "unauthorized_client") => {
            "Microsoft refused this launcher's app registration (client ID).".into()
        }
        Some(other) => format!("Microsoft sign-in failed ({other})."),
        None => "Microsoft sign-in failed.".into(),
    }
}

fn xsts_error(value: &Value, status: u16) -> String {
    match value.get("XErr").and_then(Value::as_u64) {
        Some(2_148_916_233) => "This Microsoft account has no Xbox profile yet.".into(),
        Some(2_148_916_235) => "Xbox Live is not available in this account's country.".into(),
        Some(2_148_916_236 | 2_148_916_237) => {
            "This account needs adult verification in its country.".into()
        }
        Some(2_148_916_238) => "This is a child account and needs a family group.".into(),
        _ => format!("Xbox Live refused the sign-in ({status})."),
    }
}

/// The file a signed-in account's public half goes in, beside the skins.
pub const ACCOUNTS: &str = "accounts.txt";

/// Read the public account list: `kind:name` lines, the selected one first. The kind
/// comes back as the page's own words ("Microsoft"/"Offline"), because that is what it
/// is drawn as; `write_public` takes them back.
pub fn read_public(import: &Path) -> Vec<(String, String)> {
    let Ok(text) = std::fs::read_to_string(import.join(ACCOUNTS)) else {
        return Vec::new();
    };
    let mut accounts: Vec<(String, String)> = text
        .lines()
        .filter_map(|line| line.trim_end_matches('\r').split_once(':'))
        .filter(|(kind, name)| !name.trim().is_empty() && (*kind == "msa" || *kind == "offline"))
        .map(|(kind, name)| {
            (
                if kind == "msa" { "Microsoft" } else { "Offline" }.to_string(),
                name.trim().to_string(),
            )
        })
        .collect();
    // A hand-edited file may repeat a name; the first line is the selected one.
    let mut seen: Vec<String> = Vec::new();
    accounts.retain(|(_, name)| {
        let fresh = !seen.contains(name);
        if fresh {
            seen.push(name.clone());
        }
        fresh
    });
    accounts
}

/// Write the public account list. Never a token. Kinds are the page's words
/// ("Microsoft"/"Offline") or the file's own (`msa`/`offline`); both are accepted so a
/// caller may pass back what `read_public` gave it.
pub fn write_public(import: &Path, accounts: &[(&str, &str)]) {
    let text: String = accounts
        .iter()
        .map(|(kind, name)| {
            let kind = match *kind {
                "Microsoft" | "msa" => "msa",
                _ => "offline",
            };
            format!("{kind}:{name}\n")
        })
        .collect();
    let _ = std::fs::create_dir_all(import);
    let _ = std::fs::write(import.join(ACCOUNTS), text);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_form_value_is_encoded_the_way_oauth_wants() {
        assert_eq!(encode("XboxLive.signin offline_access"), "XboxLive.signin+offline_access");
        assert_eq!(encode("a/b?c=d&e"), "a%2Fb%3Fc%3Dd%26e");
    }

    #[test]
    fn the_public_list_round_trips_and_carries_no_token() {
        let dir = std::env::temp_dir().join(format!("horizon-accounts-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        write_public(&dir, &[("msa", "Notch"), ("offline", "Player00001")]);
        assert_eq!(
            read_public(&dir),
            vec![
                ("Microsoft".to_string(), "Notch".to_string()),
                ("Offline".to_string(), "Player00001".to_string()),
            ]
        );
        let text = std::fs::read_to_string(dir.join(ACCOUNTS)).unwrap();
        assert!(!text.contains("token"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_session_file_is_what_the_game_reads() {
        let account = Account {
            name: "Notch".into(),
            uuid: "069a79f444e94726a5befca90e38aaf5".into(),
            access_token: "secret".into(),
            refresh_token: "refresh".into(),
            expires_at: 0,
        };
        let value: Value = serde_json::from_str(&session_json(&account)).unwrap();
        assert_eq!(value["user_type"], "msa");
        assert_eq!(value["name"], "Notch");
        assert_eq!(value["uuid"], "069a79f444e94726a5befca90e38aaf5");
        // The refresh token is not part of the session the game is handed.
        assert!(value.get("refresh_token").is_none());
    }
}
