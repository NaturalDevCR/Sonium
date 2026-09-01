//! User authentication — accounts, roles, JWT tokens.
//!
//! Users are stored in a JSON file alongside `sonium.toml`.  On first boot
//! (no users file) Sonium requires an initial `admin` password from the
//! operator; it never writes credentials to logs.

use std::collections::{HashMap, HashSet};
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

#[cfg(unix)]
use std::fs::File;
#[cfg(unix)]
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

use argon2::{
    password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use parking_lot::{Mutex, RwLock};
use rand::distributions::{Alphanumeric, DistString};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tracing::{info, warn};
use uuid::Uuid;

// ── Types ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    /// Full access: server config, user management, all management operations.
    Admin,
    /// Manage groups, streams, clients, volumes; cannot touch users or config.
    Operator,
    /// Read-only access to all state.
    Viewer,
}

impl std::fmt::Display for Role {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Role::Admin => write!(f, "admin"),
            Role::Operator => write!(f, "operator"),
            Role::Viewer => write!(f, "viewer"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub id: String,
    pub username: String,
    #[serde(skip_serializing)]
    pub password_hash: String,
    pub role: Role,
    pub must_change_password: bool,
    #[serde(default)]
    pub session_version: u64,
}

/// Public view of a user (no password hash).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserView {
    pub id: String,
    pub username: String,
    pub role: Role,
    pub must_change_password: bool,
}

impl From<&User> for UserView {
    fn from(u: &User) -> Self {
        UserView {
            id: u.id.clone(),
            username: u.username.clone(),
            role: u.role.clone(),
            must_change_password: u.must_change_password,
        }
    }
}

/// JWT claims payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claims {
    pub sub: String, // user id
    pub username: String,
    pub role: String,
    pub must_change_password: bool,
    pub session_version: u64,
    pub exp: usize,
}

/// Claims admitted by a one-use WebSocket ticket.
///
/// The original JWT itself is never retained by the WebSocket connection.
#[derive(Debug, Clone)]
pub struct WsTicketClaims {
    claims: Claims,
    token_hash: String,
    expires_at: Instant,
}

/// Failure while issuing a WebSocket admission ticket.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WsTicketIssueError {
    InvalidToken,
    CapacityExceeded,
}

const WS_TICKET_TTL: Duration = Duration::from_secs(60);
const MAX_PENDING_WS_TICKETS: usize = 1024;

// ── Serialisation shim for the users file ────────────────────────────────

#[derive(Serialize, Deserialize)]
struct UsersFile {
    users: Vec<UserRecord>,
    #[serde(default)]
    jwt_secret: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct UserRecord {
    id: String,
    username: String,
    password_hash: String,
    role: Role,
    #[serde(default)]
    must_change_password: bool,
    #[serde(default)]
    session_version: u64,
}

#[cfg(not(windows))]
fn replace_file(source: &Path, destination: &Path) -> std::io::Result<()> {
    std::fs::rename(source, destination)
}

#[cfg(windows)]
fn replace_file(source: &Path, destination: &Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
    };

    let source: Vec<u16> = source.as_os_str().encode_wide().chain(Some(0)).collect();
    let destination: Vec<u16> = destination
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect();
    // MoveFileExW supplies the replace-existing behavior that std::fs::rename
    // intentionally lacks on Windows. WRITE_THROUGH asks the OS not to report
    // success until the move has reached durable storage.
    let replaced = unsafe {
        MoveFileExW(
            source.as_ptr(),
            destination.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if replaced == 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

// ── UserStore ─────────────────────────────────────────────────────────────

pub struct UserStore {
    users: RwLock<HashMap<String, User>>,
    jwt_secret: RwLock<String>,
    file_path: PathBuf,
    revoked: RwLock<HashSet<String>>,
    ws_tickets: Mutex<HashMap<String, WsTicketClaims>>,
    persistence: Mutex<()>,
    #[cfg(test)]
    persist_fault: Mutex<Option<PersistFault>>,
    #[cfg(test)]
    mutation_pause: Mutex<Option<MutationPause>>,
}

#[cfg(test)]
#[derive(Clone, Copy, PartialEq, Eq)]
enum PersistFault {
    BeforeWrite,
    BeforeRename,
    AfterRename,
}

#[cfg(test)]
struct MutationPause {
    entered: Arc<std::sync::Barrier>,
    release: Arc<std::sync::Barrier>,
}

impl UserStore {
    /// Load from `<config_dir>/users.json`, or create it with a default admin
    /// if the file does not exist.
    pub fn load_or_init(
        config_dir: &Path,
        initial_password: Option<String>,
    ) -> anyhow::Result<Arc<Self>> {
        let file_path = config_dir.join("users.json");
        std::fs::create_dir_all(config_dir)?;
        Self::set_directory_permissions(config_dir)?;
        let store = Arc::new(Self {
            users: RwLock::new(HashMap::new()),
            jwt_secret: RwLock::new(Self::generate_secret()),
            file_path: file_path.clone(),
            revoked: RwLock::new(HashSet::new()),
            ws_tickets: Mutex::new(HashMap::new()),
            persistence: Mutex::new(()),
            #[cfg(test)]
            persist_fault: Mutex::new(None),
            #[cfg(test)]
            mutation_pause: Mutex::new(None),
        });

        let users_file_exists = match std::fs::metadata(&file_path) {
            Ok(_) => true,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => false,
            Err(e) => return Err(e.into()),
        };

        if users_file_exists {
            store.load_from_disk()?;
            info!(path = %file_path.display(), users = store.users.read().len(), "Loaded users");
        }

        if !users_file_exists && store.users.read().is_empty() {
            let password = initial_password.ok_or_else(|| {
                anyhow::anyhow!(
                    "no users file found; initialize an admin with --init-admin before starting"
                )
            })?;
            store.create_user_internal("admin", &password, Role::Admin, true);
            store.persist()?;
            warn!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
            warn!(" Created initial admin account.");
            warn!(" Username: admin");
            warn!(" Change the initial password in the web UI → /admin/users.");
            warn!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
        }
        // Ensure older users.json files gain a persistent JWT secret.
        if users_file_exists {
            store.persist()?;
        }

        Ok(store)
    }

    #[cfg(unix)]
    fn set_directory_permissions(config_dir: &Path) -> anyhow::Result<()> {
        std::fs::set_permissions(config_dir, std::fs::Permissions::from_mode(0o700))?;
        Ok(())
    }

    #[cfg(not(unix))]
    fn set_directory_permissions(_config_dir: &Path) -> anyhow::Result<()> {
        Ok(())
    }

    fn generate_secret() -> String {
        Alphanumeric.sample_string(&mut rand::thread_rng(), 64)
    }

    fn load_from_disk(&self) -> anyhow::Result<()> {
        let raw = std::fs::read_to_string(&self.file_path)?;
        let file: UsersFile = serde_json::from_str(&raw)?;
        let jwt_secret = match file.jwt_secret {
            Some(secret) if secret.is_empty() => {
                return Err(anyhow::anyhow!("users file contains an empty JWT secret"));
            }
            secret => secret,
        };
        let mut users = HashMap::with_capacity(file.users.len());
        let mut usernames = HashSet::with_capacity(file.users.len());
        let mut has_admin = false;
        for r in file.users {
            if !usernames.insert(r.username.clone()) {
                return Err(anyhow::anyhow!("users file contains duplicate usernames"));
            }
            let password_hash = PasswordHash::new(&r.password_hash)
                .map_err(|_| anyhow::anyhow!("users file contains an invalid password hash"))?;
            if password_hash.algorithm.as_str() != "argon2id" {
                return Err(anyhow::anyhow!(
                    "users file contains an unsupported password hash algorithm"
                ));
            }
            let user = User {
                id: r.id,
                username: r.username,
                password_hash: r.password_hash,
                role: r.role,
                must_change_password: r.must_change_password,
                session_version: r.session_version,
            };
            has_admin |= user.role == Role::Admin;
            if users.insert(user.id.clone(), user).is_some() {
                return Err(anyhow::anyhow!("users file contains duplicate user IDs"));
            }
        }
        if users.is_empty() {
            return Err(anyhow::anyhow!("users file contains no accounts"));
        }
        if !has_admin {
            return Err(anyhow::anyhow!("users file contains no admin account"));
        }
        if let Some(secret) = jwt_secret {
            *self.jwt_secret.write() = secret;
        }
        *self.users.write() = users;
        Ok(())
    }

    fn persist(&self) -> anyhow::Result<()> {
        let _persistence = self.persistence.lock();
        self.persist_locked()
    }

    fn persist_locked(&self) -> anyhow::Result<()> {
        let users = self.users.read();
        self.persist_users_locked(&users)
    }

    fn persist_users_locked(&self, users: &HashMap<String, User>) -> anyhow::Result<()> {
        let records: Vec<UserRecord> = users
            .values()
            .map(|u| UserRecord {
                id: u.id.clone(),
                username: u.username.clone(),
                password_hash: u.password_hash.clone(),
                role: u.role.clone(),
                must_change_password: u.must_change_password,
                session_version: u.session_version,
            })
            .collect();
        let file = UsersFile {
            users: records,
            jwt_secret: Some(self.jwt_secret.read().clone()),
        };
        let json = serde_json::to_vec_pretty(&file)?;
        let parent = self
            .file_path
            .parent()
            .ok_or_else(|| anyhow::anyhow!("users file has no parent directory"))?;
        let temp_path = parent.join(format!(".users-{}.tmp", Uuid::new_v4()));

        let result = (|| -> anyhow::Result<()> {
            #[cfg(test)]
            if self.take_persist_fault(PersistFault::BeforeWrite) {
                return Err(anyhow::anyhow!("injected persistence write failure"));
            }
            let mut options = OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            options.mode(0o600);
            let mut temp = options.open(&temp_path)?;
            temp.write_all(&json)?;
            temp.sync_all()?;
            drop(temp);
            #[cfg(test)]
            if self.take_persist_fault(PersistFault::BeforeRename) {
                return Err(anyhow::anyhow!("injected persistence rename failure"));
            }
            replace_file(&temp_path, &self.file_path)?;
            if let Err(error) = self.sync_parent_after_rename(parent) {
                warn!(
                    path = %parent.display(),
                    "Users file was replaced but directory fsync failed: {error}"
                );
            }
            Ok(())
        })();

        if result.is_err() {
            let _ = std::fs::remove_file(&temp_path);
        }
        result
    }

    fn sync_parent_after_rename(&self, parent: &Path) -> anyhow::Result<()> {
        #[cfg(test)]
        if self.take_persist_fault(PersistFault::AfterRename) {
            return Err(anyhow::anyhow!("injected post-rename sync failure"));
        }
        #[cfg(unix)]
        File::open(parent)?.sync_all()?;
        #[cfg(not(unix))]
        let _ = parent;
        Ok(())
    }

    #[cfg(test)]
    fn take_persist_fault(&self, expected: PersistFault) -> bool {
        let mut fault = self.persist_fault.lock();
        if *fault == Some(expected) {
            *fault = None;
            true
        } else {
            false
        }
    }

    fn hash_password(password: &str) -> anyhow::Result<String> {
        let salt = SaltString::generate(&mut OsRng);
        let hash = Argon2::default()
            .hash_password(password.as_bytes(), &salt)
            .map_err(|e| anyhow::anyhow!("hash error: {e}"))?
            .to_string();
        Ok(hash)
    }

    fn verify_password(password: &str, hash: &str) -> bool {
        let Ok(parsed) = PasswordHash::new(hash) else {
            return false;
        };
        Argon2::default()
            .verify_password(password.as_bytes(), &parsed)
            .is_ok()
    }

    fn create_user_internal(
        &self,
        username: &str,
        password: &str,
        role: Role,
        must_change: bool,
    ) -> User {
        let hash = Self::hash_password(password).expect("argon2 hash failed");
        let user = User {
            id: Uuid::new_v4().to_string(),
            username: username.to_owned(),
            password_hash: hash,
            role,
            must_change_password: must_change,
            session_version: 0,
        };
        self.users.write().insert(user.id.clone(), user.clone());
        user
    }

    // ── Public API ────────────────────────────────────────────────────────

    pub fn is_setup_needed(&self) -> bool {
        self.users.read().is_empty()
    }

    pub fn authenticate(&self, username: &str, password: &str) -> Option<User> {
        let users = self.users.read();
        users
            .values()
            .find(|u| u.username == username && Self::verify_password(password, &u.password_hash))
            .cloned()
    }

    pub fn create_token(&self, user: &User, ttl_hours: u64) -> String {
        use std::time::{SystemTime, UNIX_EPOCH};
        let exp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as usize
            + (ttl_hours * 3600) as usize;

        let claims = Claims {
            sub: user.id.clone(),
            username: user.username.clone(),
            role: user.role.to_string(),
            must_change_password: user.must_change_password,
            session_version: user.session_version,
            exp,
        };
        encode(
            &Header::default(),
            &claims,
            &EncodingKey::from_secret(self.jwt_secret.read().as_bytes()),
        )
        .expect("JWT encode failed")
    }

    fn token_hash(token: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(token.as_bytes());
        format!("{:x}", hasher.finalize())
    }

    /// Revoke a token. It will be rejected by `verify_token` until server restart.
    pub fn revoke_token(&self, token: &str) {
        self.revoked.write().insert(Self::token_hash(token));
    }

    pub fn verify_token(&self, token: &str) -> Option<Claims> {
        if self.revoked.read().contains(&Self::token_hash(token)) {
            return None;
        }
        let claims = decode::<Claims>(
            token,
            &DecodingKey::from_secret(self.jwt_secret.read().as_bytes()),
            &Validation::default(),
        )
        .ok()
        .map(|d| d.claims)?;
        let user = self.users.read().get(&claims.sub).cloned()?;
        (user.role.to_string() == claims.role && user.session_version == claims.session_version)
            .then_some(claims)
    }

    /// Issue a random, short-lived, single-use ticket for a WebSocket upgrade.
    pub fn issue_ws_ticket(&self, token: &str) -> Result<String, WsTicketIssueError> {
        self.issue_ws_ticket_with_ttl(token, WS_TICKET_TTL)
    }

    fn issue_ws_ticket_with_ttl(
        &self,
        token: &str,
        ttl: Duration,
    ) -> Result<String, WsTicketIssueError> {
        let claims = self
            .verify_token(token)
            .ok_or(WsTicketIssueError::InvalidToken)?;
        let mut tickets = self.ws_tickets.lock();
        let now = Instant::now();
        tickets.retain(|_, ticket| ticket.expires_at > now);
        if tickets.len() >= MAX_PENDING_WS_TICKETS {
            return Err(WsTicketIssueError::CapacityExceeded);
        }

        for _ in 0..4 {
            let ticket = Alphanumeric.sample_string(&mut rand::thread_rng(), 48);
            if tickets.contains_key(&ticket) {
                continue;
            }
            tickets.insert(
                ticket.clone(),
                WsTicketClaims {
                    claims,
                    token_hash: Self::token_hash(token),
                    expires_at: now + ttl,
                },
            );
            return Ok(ticket);
        }
        Err(WsTicketIssueError::CapacityExceeded)
    }

    /// Atomically consume a WebSocket ticket before accepting an upgrade.
    pub fn consume_ws_ticket(&self, ticket: &str) -> Option<WsTicketClaims> {
        let ticket = self.ws_tickets.lock().remove(ticket)?;
        (ticket.expires_at > Instant::now() && self.verify_ws_ticket_claims(&ticket))
            .then_some(ticket)
    }

    /// Check that a connected WebSocket's admitted session remains valid.
    pub fn verify_ws_ticket_claims(&self, ticket: &WsTicketClaims) -> bool {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(usize::MAX, |duration| duration.as_secs() as usize);
        if ticket.claims.exp <= now {
            return false;
        }
        if self.revoked.read().contains(&ticket.token_hash) {
            return false;
        }
        self.users
            .read()
            .get(&ticket.claims.sub)
            .is_some_and(|user| {
                user.role.to_string() == ticket.claims.role
                    && user.session_version == ticket.claims.session_version
            })
    }

    pub fn all_users(&self) -> Vec<UserView> {
        self.users.read().values().map(UserView::from).collect()
    }

    pub fn get_user(&self, id: &str) -> Option<UserView> {
        self.users.read().get(id).map(UserView::from)
    }

    /// Create a new user. Returns `Ok(None)` if the username is already taken.
    pub fn create_user(
        &self,
        username: &str,
        password: &str,
        role: Role,
    ) -> anyhow::Result<Option<UserView>> {
        let _persistence = self.persistence.lock();
        let mut users = self.users.write();
        if users.values().any(|u| u.username == username) {
            return Ok(None);
        }
        let user = User {
            id: Uuid::new_v4().to_string(),
            username: username.to_owned(),
            password_hash: Self::hash_password(password)?,
            role,
            must_change_password: false,
            session_version: 0,
        };
        users.insert(user.id.clone(), user.clone());
        if let Err(error) = self.persist_users_locked(&users) {
            users.remove(&user.id);
            return Err(error);
        }
        drop(users);
        info!(username, id = %user.id, "User created");
        Ok(Some(UserView::from(&user)))
    }

    /// Update a user's role and/or password. Returns `Ok(false)` if not found.
    pub fn update_user(
        &self,
        id: &str,
        role: Option<Role>,
        new_password: Option<&str>,
    ) -> anyhow::Result<bool> {
        let _persistence = self.persistence.lock();
        let mut users = self.users.write();
        let Some(original) = users.get(id).cloned() else {
            return Ok(false);
        };
        let mut updated = original.clone();
        let mut invalidate_sessions = false;
        if let Some(role) = role {
            if updated.role != role {
                updated.role = role;
                invalidate_sessions = true;
            }
        }
        if let Some(password) = new_password {
            updated.password_hash = Self::hash_password(password)?;
            updated.must_change_password = false;
            invalidate_sessions = true;
        }
        if invalidate_sessions {
            updated.session_version = original
                .session_version
                .checked_add(1)
                .ok_or_else(|| anyhow::anyhow!("session version exhausted"))?;
        }
        users.insert(id.to_owned(), updated);
        #[cfg(test)]
        self.pause_after_mutation();
        if let Err(error) = self.persist_users_locked(&users) {
            users.insert(id.to_owned(), original);
            return Err(error);
        }
        drop(users);
        Ok(true)
    }

    /// Delete a user. Returns `Ok(false)` if not found or deleting the last admin.
    pub fn delete_user(&self, id: &str) -> anyhow::Result<bool> {
        let _persistence = self.persistence.lock();
        let mut users = self.users.write();
        if let Some(u) = users.get(id) {
            if u.role == Role::Admin {
                let admin_count = users.values().filter(|x| x.role == Role::Admin).count();
                if admin_count <= 1 {
                    warn!("Cannot delete the last admin account");
                    return Ok(false);
                }
            }
        } else {
            return Ok(false);
        }
        let deleted = users.remove(id).expect("checked user exists");
        if let Err(error) = self.persist_users_locked(&users) {
            users.insert(id.to_owned(), deleted);
            return Err(error);
        }
        drop(users);
        Ok(true)
    }
}

#[cfg(test)]
impl UserStore {
    fn fail_next_persist(&self, fault: PersistFault) {
        *self.persist_fault.lock() = Some(fault);
    }

    fn pause_after_next_mutation(
        &self,
        entered: Arc<std::sync::Barrier>,
        release: Arc<std::sync::Barrier>,
    ) {
        *self.mutation_pause.lock() = Some(MutationPause { entered, release });
    }

    fn pause_after_mutation(&self) {
        if let Some(pause) = self.mutation_pause.lock().take() {
            pause.entered.wait();
            pause.release.wait();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    use tracing::field::{Field, Visit};
    use tracing::span::{Attributes, Id, Record};
    use tracing::{Event, Metadata, Subscriber};

    struct LogCapture {
        output: Arc<Mutex<String>>,
    }

    struct LogVisitor<'a> {
        output: &'a Mutex<String>,
    }

    impl Visit for LogVisitor<'_> {
        fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
            self.output
                .lock()
                .push_str(&format!("{}={value:?}\n", field.name()));
        }
    }

    impl Subscriber for LogCapture {
        fn enabled(&self, _: &Metadata<'_>) -> bool {
            true
        }

        fn new_span(&self, _: &Attributes<'_>) -> Id {
            Id::from_u64(1)
        }

        fn record(&self, _: &Id, _: &Record<'_>) {}

        fn record_follows_from(&self, _: &Id, _: &Id) {}

        fn event(&self, event: &Event<'_>) {
            event.record(&mut LogVisitor {
                output: &self.output,
            });
        }

        fn enter(&self, _: &Id) {}

        fn exit(&self, _: &Id) {}
    }

    #[test]
    fn initial_admin_password_authenticates() {
        let dir = tempdir().unwrap();
        let store =
            UserStore::load_or_init(dir.path(), Some("generated-pass".to_string())).unwrap();

        // Verify it was created
        {
            let users = store.users.read();
            assert_eq!(users.len(), 1);
            let admin = users.values().next().unwrap();
            assert_eq!(admin.username, "admin");
            assert!(admin.must_change_password);
        }

        // Verify authentication
        let auth = store.authenticate("admin", "generated-pass");
        assert!(
            auth.is_some(),
            "Authentication should succeed with generated password"
        );
    }

    #[test]
    fn test_raw_argon2_logic() {
        let password = "testpassword123";
        let salt = SaltString::generate(&mut rand::rngs::OsRng);
        let argon2 = Argon2::default();
        let hash = argon2
            .hash_password(password.as_bytes(), &salt)
            .unwrap()
            .to_string();

        let parsed_hash = PasswordHash::new(&hash).unwrap();
        assert!(argon2
            .verify_password(password.as_bytes(), &parsed_hash)
            .is_ok());
    }

    #[test]
    fn initial_admin_password_persists_in_new_config_dir() {
        let root = tempdir().unwrap();
        let config_dir = root.path().join("nested/config");

        let store =
            UserStore::load_or_init(&config_dir, Some("generated-pass".to_string())).unwrap();
        assert!(store.authenticate("admin", "generated-pass").is_some());
        assert!(config_dir.join("users.json").exists());

        let reloaded = UserStore::load_or_init(&config_dir, None).unwrap();
        assert!(reloaded.authenticate("admin", "generated-pass").is_some());
    }

    #[test]
    fn corrupt_existing_account_file_is_not_replaced() {
        let dir = tempdir().unwrap();
        let users_file = dir.path().join("users.json");
        let corrupt_contents = b"{ this is not valid json";
        std::fs::write(&users_file, corrupt_contents).unwrap();

        let result = UserStore::load_or_init(dir.path(), Some("generated-pass".to_string()));
        assert!(
            result.is_err(),
            "a corrupt existing account file must prevent startup"
        );
        let error = result.err().unwrap();

        assert!(error.to_string().contains("key must be a string"));
        assert_eq!(std::fs::read(&users_file).unwrap(), corrupt_contents);
    }

    #[test]
    fn missing_account_file_requires_an_initial_admin_password() {
        let dir = tempdir().unwrap();

        let result = UserStore::load_or_init(dir.path(), None);

        assert!(result.is_err());
        assert!(!dir.path().join("users.json").exists());
    }

    #[test]
    fn initial_admin_password_is_never_logged() {
        let dir = tempdir().unwrap();
        let logs = Arc::new(Mutex::new(String::new()));
        let dispatch = tracing::Dispatch::new(LogCapture {
            output: logs.clone(),
        });

        tracing::dispatcher::with_default(&dispatch, || {
            UserStore::load_or_init(dir.path(), Some("password-not-for-logs".to_string())).unwrap();
        });

        assert!(!logs.lock().contains("password-not-for-logs"));
    }

    #[test]
    fn legacy_account_files_default_the_session_version() {
        let dir = tempdir().unwrap();
        let store =
            UserStore::load_or_init(dir.path(), Some("admin-password".to_string())).unwrap();
        drop(store);
        let users_file = dir.path().join("users.json");
        let mut file: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&users_file).unwrap()).unwrap();
        file["users"][0]
            .as_object_mut()
            .unwrap()
            .remove("session_version");
        std::fs::write(&users_file, serde_json::to_vec(&file).unwrap()).unwrap();

        let reloaded = UserStore::load_or_init(dir.path(), None).unwrap();

        assert!(reloaded.authenticate("admin", "admin-password").is_some());
    }

    #[cfg(unix)]
    #[test]
    fn account_store_uses_owner_only_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let root = tempdir().unwrap();
        let config_dir = root.path().join("auth");
        let _ = UserStore::load_or_init(&config_dir, Some("admin-password".to_string())).unwrap();

        assert_eq!(
            std::fs::metadata(&config_dir).unwrap().permissions().mode() & 0o777,
            0o700
        );
        assert_eq!(
            std::fs::metadata(config_dir.join("users.json"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }

    #[test]
    fn password_change_invalidates_existing_tokens() {
        let dir = tempdir().unwrap();
        let store =
            UserStore::load_or_init(dir.path(), Some("admin-password".to_string())).unwrap();
        let user = store
            .create_user("alice", "initial-password", Role::Viewer)
            .unwrap()
            .unwrap();
        let token =
            store.create_token(&store.authenticate("alice", "initial-password").unwrap(), 1);

        assert!(store
            .update_user(&user.id, None, Some("new-password"))
            .unwrap());

        assert!(store.verify_token(&token).is_none());
    }

    #[test]
    fn password_change_invalidation_survives_reload() {
        let dir = tempdir().unwrap();
        let store =
            UserStore::load_or_init(dir.path(), Some("admin-password".to_string())).unwrap();
        let user = store
            .create_user("alice", "initial-password", Role::Viewer)
            .unwrap()
            .unwrap();
        let token =
            store.create_token(&store.authenticate("alice", "initial-password").unwrap(), 1);

        assert!(store
            .update_user(&user.id, None, Some("new-password"))
            .unwrap());
        drop(store);

        let reloaded = UserStore::load_or_init(dir.path(), None).unwrap();

        assert!(reloaded.verify_token(&token).is_none());
    }

    #[test]
    fn role_change_invalidates_existing_tokens() {
        let dir = tempdir().unwrap();
        let store =
            UserStore::load_or_init(dir.path(), Some("admin-password".to_string())).unwrap();
        let user = store
            .create_user("alice", "alice-password", Role::Viewer)
            .unwrap()
            .unwrap();
        let token = store.create_token(&store.authenticate("alice", "alice-password").unwrap(), 1);

        assert!(store
            .update_user(&user.id, Some(Role::Operator), None)
            .unwrap());

        assert!(store.verify_token(&token).is_none());
    }

    #[test]
    fn deleted_user_tokens_are_invalidated() {
        let dir = tempdir().unwrap();
        let store =
            UserStore::load_or_init(dir.path(), Some("admin-password".to_string())).unwrap();
        let user = store
            .create_user("alice", "alice-password", Role::Viewer)
            .unwrap()
            .unwrap();
        let token = store.create_token(&store.authenticate("alice", "alice-password").unwrap(), 1);

        assert!(store.delete_user(&user.id).unwrap());

        assert!(store.verify_token(&token).is_none());
    }

    #[test]
    fn failed_update_does_not_invalidate_or_persist_sessions() {
        let dir = tempdir().unwrap();
        let store =
            UserStore::load_or_init(dir.path(), Some("admin-password".to_string())).unwrap();
        let user = store
            .create_user("alice", "initial-password", Role::Viewer)
            .unwrap()
            .unwrap();
        let token =
            store.create_token(&store.authenticate("alice", "initial-password").unwrap(), 1);
        store.fail_next_persist(PersistFault::BeforeRename);

        assert!(store
            .update_user(&user.id, Some(Role::Operator), None)
            .is_err());
        assert!(store.verify_token(&token).is_some());
        drop(store);

        let reloaded = UserStore::load_or_init(dir.path(), None).unwrap();
        assert!(reloaded.verify_token(&token).is_some());
    }

    #[test]
    fn failed_delete_does_not_remove_or_persist_user() {
        let dir = tempdir().unwrap();
        let store =
            UserStore::load_or_init(dir.path(), Some("admin-password".to_string())).unwrap();
        let user = store
            .create_user("alice", "alice-password", Role::Viewer)
            .unwrap()
            .unwrap();
        let token = store.create_token(&store.authenticate("alice", "alice-password").unwrap(), 1);
        store.fail_next_persist(PersistFault::BeforeRename);

        assert!(store.delete_user(&user.id).is_err());
        assert!(store.verify_token(&token).is_some());
        drop(store);

        let reloaded = UserStore::load_or_init(dir.path(), None).unwrap();
        assert!(reloaded.verify_token(&token).is_some());
    }

    #[test]
    fn failed_create_does_not_add_user() {
        let dir = tempdir().unwrap();
        let store =
            UserStore::load_or_init(dir.path(), Some("admin-password".to_string())).unwrap();
        store.fail_next_persist(PersistFault::BeforeRename);

        assert!(store
            .create_user("alice", "alice-password", Role::Viewer)
            .is_err());
        assert!(store.authenticate("alice", "alice-password").is_none());
    }

    #[test]
    fn failed_atomic_replace_preserves_previous_bytes_and_permissions() {
        #[cfg(unix)]
        use std::os::unix::fs::PermissionsExt;

        let dir = tempdir().unwrap();
        let store =
            UserStore::load_or_init(dir.path(), Some("admin-password".to_string())).unwrap();
        let user = store
            .create_user("alice", "alice-password", Role::Viewer)
            .unwrap()
            .unwrap();
        let users_file = dir.path().join("users.json");
        let old_bytes = std::fs::read(&users_file).unwrap();
        #[cfg(unix)]
        let old_permissions = std::fs::metadata(&users_file).unwrap().permissions().mode() & 0o777;
        for fault in [PersistFault::BeforeWrite, PersistFault::BeforeRename] {
            store.fail_next_persist(fault);

            assert!(store
                .update_user(&user.id, Some(Role::Operator), None)
                .is_err());

            assert_eq!(std::fs::read(&users_file).unwrap(), old_bytes);
            #[cfg(unix)]
            assert_eq!(
                std::fs::metadata(&users_file).unwrap().permissions().mode() & 0o777,
                old_permissions
            );
        }
    }

    #[test]
    fn atomic_replace_overwrites_an_existing_destination() {
        let dir = tempdir().unwrap();
        let destination = dir.path().join("users.json");
        let replacement = dir.path().join("users.next.json");
        std::fs::write(&destination, b"old").unwrap();
        std::fs::write(&replacement, b"new").unwrap();

        replace_file(&replacement, &destination).unwrap();

        assert_eq!(std::fs::read(&destination).unwrap(), b"new");
        assert!(!replacement.exists());
    }

    #[test]
    fn rejects_duplicate_ids_invalid_hashes_and_empty_jwt_secrets_without_rewrite() {
        enum InvalidCase {
            DuplicateId,
            InvalidHash,
            EmptySecret,
        }

        for case in [
            InvalidCase::DuplicateId,
            InvalidCase::InvalidHash,
            InvalidCase::EmptySecret,
        ] {
            let dir = tempdir().unwrap();
            let store =
                UserStore::load_or_init(dir.path(), Some("admin-password".to_string())).unwrap();
            drop(store);
            let users_file = dir.path().join("users.json");
            let mut file: serde_json::Value =
                serde_json::from_slice(&std::fs::read(&users_file).unwrap()).unwrap();
            match case {
                InvalidCase::DuplicateId => {
                    let duplicate = file["users"][0].clone();
                    file["users"].as_array_mut().unwrap().push(duplicate);
                }
                InvalidCase::InvalidHash => {
                    file["users"][0]["password_hash"] = serde_json::json!("not-a-phc-hash");
                }
                InvalidCase::EmptySecret => file["jwt_secret"] = serde_json::json!(""),
            }
            let invalid_bytes = serde_json::to_vec(&file).unwrap();
            std::fs::write(&users_file, &invalid_bytes).unwrap();

            assert!(UserStore::load_or_init(dir.path(), None).is_err());
            assert_eq!(std::fs::read(&users_file).unwrap(), invalid_bytes);
        }
    }

    #[test]
    fn rejects_empty_no_admin_and_duplicate_username_files_without_rewrite() {
        enum InvalidCase {
            Empty,
            NoAdmin,
            DuplicateUsername,
        }

        for case in [
            InvalidCase::Empty,
            InvalidCase::NoAdmin,
            InvalidCase::DuplicateUsername,
        ] {
            let dir = tempdir().unwrap();
            let store =
                UserStore::load_or_init(dir.path(), Some("admin-password".to_string())).unwrap();
            let viewer = store
                .create_user("alice", "alice-password", Role::Viewer)
                .unwrap()
                .unwrap();
            drop(store);
            let users_file = dir.path().join("users.json");
            let mut file: serde_json::Value =
                serde_json::from_slice(&std::fs::read(&users_file).unwrap()).unwrap();
            match case {
                InvalidCase::Empty => file["users"] = serde_json::json!([]),
                InvalidCase::NoAdmin => {
                    file["users"].as_array_mut().unwrap().retain(|user| {
                        user["role"] != serde_json::Value::String("admin".to_owned())
                    });
                }
                InvalidCase::DuplicateUsername => {
                    let duplicate = file["users"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .find(|user| user["id"] == viewer.id)
                        .unwrap()
                        .clone();
                    let mut duplicate = duplicate;
                    duplicate["id"] = serde_json::json!(Uuid::new_v4().to_string());
                    file["users"].as_array_mut().unwrap().push(duplicate);
                }
            }
            let invalid_bytes = serde_json::to_vec(&file).unwrap();
            std::fs::write(&users_file, &invalid_bytes).unwrap();

            assert!(
                UserStore::load_or_init(dir.path(), None).is_err(),
                "semantically invalid users file must prevent startup"
            );
            assert_eq!(std::fs::read(&users_file).unwrap(), invalid_bytes);
        }
    }

    #[test]
    fn rejects_non_argon2id_password_hashes_without_rewrite() {
        let dir = tempdir().unwrap();
        let store =
            UserStore::load_or_init(dir.path(), Some("admin-password".to_string())).unwrap();
        drop(store);
        let users_file = dir.path().join("users.json");
        let mut file: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&users_file).unwrap()).unwrap();
        let hash = file["users"][0]["password_hash"].as_str().unwrap();
        file["users"][0]["password_hash"] =
            serde_json::json!(hash.replacen("$argon2id$", "$argon2i$", 1,));
        let invalid_bytes = serde_json::to_vec(&file).unwrap();
        std::fs::write(&users_file, &invalid_bytes).unwrap();

        assert!(UserStore::load_or_init(dir.path(), None).is_err());
        assert_eq!(std::fs::read(&users_file).unwrap(), invalid_bytes);
    }

    #[test]
    fn session_version_exhaustion_rejects_the_update_without_invalidating_tokens() {
        let dir = tempdir().unwrap();
        let store =
            UserStore::load_or_init(dir.path(), Some("admin-password".to_string())).unwrap();
        let user = store
            .create_user("alice", "alice-password", Role::Viewer)
            .unwrap()
            .unwrap();
        store
            .users
            .write()
            .get_mut(&user.id)
            .unwrap()
            .session_version = u64::MAX;
        let token = store.create_token(&store.authenticate("alice", "alice-password").unwrap(), 1);

        assert!(store
            .update_user(&user.id, Some(Role::Operator), None)
            .is_err());
        assert!(store.verify_token(&token).is_some());
    }

    #[test]
    fn concurrent_mutations_wait_for_the_persistence_lock() {
        use std::sync::mpsc;
        use std::time::Duration;

        let dir = tempdir().unwrap();
        let store =
            UserStore::load_or_init(dir.path(), Some("admin-password".to_string())).unwrap();
        let user = store
            .create_user("alice", "alice-password", Role::Viewer)
            .unwrap()
            .unwrap();
        let other_user = store
            .create_user("bob", "bob-password", Role::Viewer)
            .unwrap()
            .unwrap();
        let lock = store.persistence.lock();
        let (done_tx, done_rx) = mpsc::channel();
        let first_done_tx = done_tx.clone();
        let store_for_thread = store.clone();
        let id = user.id.clone();
        let worker = std::thread::spawn(move || {
            let result = store_for_thread.update_user(&id, Some(Role::Operator), None);
            first_done_tx.send(result).unwrap();
        });
        let store_for_second_thread = store.clone();
        let other_id = other_user.id.clone();
        let second_worker = std::thread::spawn(move || {
            let result = store_for_second_thread.update_user(&other_id, Some(Role::Operator), None);
            done_tx.send(result).unwrap();
        });

        assert!(done_rx.recv_timeout(Duration::from_millis(50)).is_err());
        drop(lock);
        for _ in 0..2 {
            assert!(done_rx
                .recv_timeout(Duration::from_secs(1))
                .unwrap()
                .unwrap());
        }
        worker.join().unwrap();
        second_worker.join().unwrap();
        drop(store);

        let reloaded = UserStore::load_or_init(dir.path(), None).unwrap();
        assert_eq!(reloaded.get_user(&user.id).unwrap().role, Role::Operator);
        assert_eq!(
            reloaded.get_user(&other_user.id).unwrap().role,
            Role::Operator
        );
    }

    #[test]
    fn failed_mutation_is_never_visible_to_token_readers() {
        use std::sync::mpsc;
        use std::time::Duration;

        let dir = tempdir().unwrap();
        let store =
            UserStore::load_or_init(dir.path(), Some("admin-password".to_string())).unwrap();
        let user = store
            .create_user("alice", "alice-password", Role::Viewer)
            .unwrap()
            .unwrap();
        let token = store.create_token(&store.authenticate("alice", "alice-password").unwrap(), 1);
        let entered = Arc::new(std::sync::Barrier::new(2));
        let release = Arc::new(std::sync::Barrier::new(2));
        store.pause_after_next_mutation(entered.clone(), release.clone());
        store.fail_next_persist(PersistFault::BeforeRename);
        let (update_tx, update_rx) = mpsc::channel();
        let updating_store = store.clone();
        let id = user.id.clone();
        let update = std::thread::spawn(move || {
            update_tx
                .send(updating_store.update_user(&id, Some(Role::Operator), None))
                .unwrap();
        });

        entered.wait();
        let (verify_tx, verify_rx) = mpsc::channel();
        let verifying_store = store.clone();
        let verify = std::thread::spawn(move || {
            verify_tx
                .send(verifying_store.verify_token(&token))
                .unwrap();
        });

        let observed_while_paused = verify_rx.recv_timeout(Duration::from_millis(50));
        release.wait();
        assert!(observed_while_paused.is_err());
        assert!(update_rx
            .recv_timeout(Duration::from_secs(1))
            .unwrap()
            .is_err());
        assert!(verify_rx
            .recv_timeout(Duration::from_secs(1))
            .unwrap()
            .is_some());
        update.join().unwrap();
        verify.join().unwrap();
    }

    #[test]
    fn post_rename_sync_failure_keeps_the_committed_state() {
        let dir = tempdir().unwrap();
        let store =
            UserStore::load_or_init(dir.path(), Some("admin-password".to_string())).unwrap();
        let user = store
            .create_user("alice", "alice-password", Role::Viewer)
            .unwrap()
            .unwrap();
        store.fail_next_persist(PersistFault::AfterRename);

        assert!(store
            .update_user(&user.id, Some(Role::Operator), None)
            .is_ok());
        assert_eq!(store.get_user(&user.id).unwrap().role, Role::Operator);
        drop(store);

        let reloaded = UserStore::load_or_init(dir.path(), None).unwrap();
        assert_eq!(reloaded.get_user(&user.id).unwrap().role, Role::Operator);
    }

    #[test]
    fn websocket_ticket_expires_and_is_consumed_once() {
        let dir = tempdir().unwrap();
        let store =
            UserStore::load_or_init(dir.path(), Some("admin-password".to_string())).unwrap();
        let token = store.create_token(&store.authenticate("admin", "admin-password").unwrap(), 1);

        let expired = store
            .issue_ws_ticket_with_ttl(&token, std::time::Duration::ZERO)
            .expect("valid token issues a ticket");
        assert!(
            store.consume_ws_ticket(&expired).is_none(),
            "expired ticket is rejected"
        );

        let ticket = store
            .issue_ws_ticket(&token)
            .expect("valid token issues a ticket");
        assert!(
            store.consume_ws_ticket(&ticket).is_some(),
            "fresh ticket is accepted"
        );
        assert!(
            store.consume_ws_ticket(&ticket).is_none(),
            "ticket replay is rejected"
        );
    }

    #[test]
    fn websocket_ticket_is_invalidated_when_its_session_changes() {
        let dir = tempdir().unwrap();
        let store =
            UserStore::load_or_init(dir.path(), Some("admin-password".to_string())).unwrap();
        let user = store
            .create_user("alice", "alice-password", Role::Viewer)
            .unwrap()
            .unwrap();
        let token = store.create_token(&store.authenticate("alice", "alice-password").unwrap(), 1);
        let ticket = store
            .issue_ws_ticket(&token)
            .expect("valid token issues a ticket");

        assert!(store
            .update_user(&user.id, Some(Role::Operator), None)
            .unwrap());
        assert!(
            store.consume_ws_ticket(&ticket).is_none(),
            "tickets from the previous session version are rejected"
        );
    }

    #[test]
    fn websocket_ticket_claims_reject_an_expired_jwt_after_upgrade() {
        let dir = tempdir().unwrap();
        let store =
            UserStore::load_or_init(dir.path(), Some("admin-password".to_string())).unwrap();
        let user = store.authenticate("admin", "admin-password").unwrap();
        let ticket = WsTicketClaims {
            claims: Claims {
                sub: user.id,
                username: user.username,
                role: user.role.to_string(),
                must_change_password: user.must_change_password,
                session_version: user.session_version,
                exp: 0,
            },
            token_hash: "not-revoked".into(),
            expires_at: Instant::now() + Duration::from_secs(60),
        };

        assert!(
            !store.verify_ws_ticket_claims(&ticket),
            "a connected WebSocket must be rejected when its JWT expires"
        );
    }
}
