use sonium_control::auth::{UserStore, Role};
use std::path::Path;

fn main() {
    let config_dir = Path::new("/tmp/sonium-test");
    let store = UserStore::load_or_init(config_dir, None);
    
    let user = store.authenticate("admin", "testpass");
    println!("Auth result: {:?}", user);
    
    if user.is_some() {
        println!("SUCCESS: Authentication worked!");
    } else {
        println!("FAILURE: Authentication failed!");
    }
}
