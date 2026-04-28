//! User authentication — accounts, roles, JWT tokens.
//!
//! Users are stored in a JSON file alongside `sonium.toml`.  On first boot
//! (no users file) Sonium creates a default `admin` account and prints the
//! generated password to the log **once** — the user should change it via the
//! web UI immediately.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use argon2::{
    password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use parking_lot::RwLock;
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
    pub exp: usize,
}

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
}

// ── UserStore ─────────────────────────────────────────────────────────────

pub struct UserStore {
    users: RwLock<HashMap<String, User>>,
    jwt_secret: RwLock<String>,
    file_path: PathBuf,
    revoked: RwLock<HashSet<String>>,
}

impl UserStore {
    /// Load from `<config_dir>/users.json`, or create it with a default admin
    /// if the file does not exist.
    pub fn load_or_init(config_dir: &Path, initial_password: Option<String>) -> Arc<Self> {
        let file_path = config_dir.join("users.json");
        if let Err(e) = std::fs::create_dir_all(config_dir) {
            warn!(
                path = %config_dir.display(),
                "Failed to create auth config directory: {e}"
            );
        }
        let store = Arc::new(Self {
            users: RwLock::new(HashMap::new()),
            jwt_secret: RwLock::new(Self::generate_secret()),
            file_path: file_path.clone(),
            revoked: RwLock::new(HashSet::new()),
        });

        if file_path.exists() {
            if let Err(e) = store.load_from_disk() {
                warn!("Failed to load users file: {e} — starting with empty user list");
            } else {
                info!(path = %file_path.display(), users = store.users.read().len(), "Loaded users");
            }
        }

        if store.users.read().is_empty() {
            let password = initial_password
                .unwrap_or_else(|| Alphanumeric.sample_string(&mut rand::thread_rng(), 16));
            store.create_user_internal("admin", &password, Role::Admin, true);
            store.persist_or_warn("Failed to persist generated admin account");
            warn!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
            warn!(" No users found — created default admin account.");
            warn!(" Username: admin");
            warn!(" Password: {password}");
            warn!(" Change this immediately in the web UI → /admin/users");
            warn!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
        }
        // Ensure older users.json files gain a persistent JWT secret.
        store.persist_or_warn("Failed to persist users file");

        store
    }

    fn generate_secret() -> String {
        Alphanumeric.sample_string(&mut rand::thread_rng(), 64)
    }

    fn load_from_disk(&self) -> anyhow::Result<()> {
        let raw = std::fs::read_to_string(&self.file_path)?;
        let file: UsersFile = serde_json::from_str(&raw)?;
        if let Some(secret) = file.jwt_secret {
            *self.jwt_secret.write() = secret;
        }
        let mut map = self.users.write();
        for r in file.users {
            map.insert(
                r.id.clone(),
                User {
                    id: r.id,
                    username: r.username,
                    password_hash: r.password_hash,
                    role: r.role,
                    must_change_password: r.must_change_password,
                },
            );
        }
        Ok(())
    }

    fn persist(&self) -> anyhow::Result<()> {
        let records: Vec<UserRecord> = self
            .users
            .read()
            .values()
            .map(|u| UserRecord {
                id: u.id.clone(),
                username: u.username.clone(),
                password_hash: u.password_hash.clone(),
                role: u.role.clone(),
                must_change_password: u.must_change_password,
            })
            .collect();
        let file = UsersFile {
            users: records,
            jwt_secret: Some(self.jwt_secret.read().clone()),
        };
        let json = serde_json::to_string_pretty(&file)?;
        std::fs::write(&self.file_path, json)?;
        Ok(())
    }

    fn persist_or_warn(&self, message: &str) {
        if let Err(e) = self.persist() {
            warn!(path = %self.file_path.display(), "{message}: {e}");
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
        decode::<Claims>(
            token,
            &DecodingKey::from_secret(self.jwt_secret.read().as_bytes()),
            &Validation::default(),
        )
        .ok()
        .map(|d| d.claims)
    }

    pub fn all_users(&self) -> Vec<UserView> {
        self.users.read().values().map(UserView::from).collect()
    }

    pub fn get_user(&self, id: &str) -> Option<UserView> {
        self.users.read().get(id).map(UserView::from)
    }

    /// Create a new user. Returns `None` if the username is already taken.
    pub fn create_user(&self, username: &str, password: &str, role: Role) -> Option<UserView> {
        if self.users.read().values().any(|u| u.username == username) {
            return None;
        }
        let user = self.create_user_internal(username, password, role, false);
        self.persist_or_warn("Failed to persist created user");
        info!(username, id = %user.id, "User created");
        Some(UserView::from(&user))
    }

    /// Update a user's role and/or password.  Returns `false` if not found.
    pub fn update_user(&self, id: &str, role: Option<Role>, new_password: Option<&str>) -> bool {
        let mut users = self.users.write();
        if let Some(u) = users.get_mut(id) {
            if let Some(r) = role {
                u.role = r;
            }
            if let Some(p) = new_password {
                u.password_hash = Self::hash_password(p).expect("hash failed");
                u.must_change_password = false;
            }
            drop(users);
            self.persist_or_warn("Failed to persist updated user");
            true
        } else {
            false
        }
    }

    /// Delete a user. Returns `false` if not found or if deleting the last admin.
    pub fn delete_user(&self, id: &str) -> bool {
        let users = self.users.read();
        if let Some(u) = users.get(id) {
            if u.role == Role::Admin {
                let admin_count = users.values().filter(|x| x.role == Role::Admin).count();
                if admin_count <= 1 {
                    warn!("Cannot delete the last admin account");
                    return false;
                }
            }
        } else {
            return false;
        }
        drop(users);
        self.users.write().remove(id);
        self.persist_or_warn("Failed to persist deleted user");
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_auto_generated_password_auth() {
        let dir = tempdir().unwrap();
        let store = UserStore::load_or_init(dir.path(), Some("generated-pass".to_string()));
        
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
        assert!(auth.is_some(), "Authentication should succeed with generated password");
    }

    #[test]
    fn test_raw_argon2_logic() {
        let password = "testpassword123";
        let salt = SaltString::generate(&mut rand::rngs::OsRng);
        let argon2 = Argon2::default();
        let hash = argon2.hash_password(password.as_bytes(), &salt).unwrap().to_string();
        
        let parsed_hash = PasswordHash::new(&hash).unwrap();
        assert!(argon2.verify_password(password.as_bytes(), &parsed_hash).is_ok());
    }

    #[test]
    fn test_auto_generated_password_persists_in_new_config_dir() {
        let root = tempdir().unwrap();
        let config_dir = root.path().join("nested/config");

        let store = UserStore::load_or_init(&config_dir, Some("generated-pass".to_string()));
        assert!(store.authenticate("admin", "generated-pass").is_some());
        assert!(config_dir.join("users.json").exists());

        let reloaded = UserStore::load_or_init(&config_dir, None);
        assert!(reloaded.authenticate("admin", "generated-pass").is_some());
    }
}
