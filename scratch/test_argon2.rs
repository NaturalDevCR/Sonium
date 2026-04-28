use argon2::{
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use rand::rngs::OsRng;

fn main() {
    let password = "testpassword123";
    let salt = SaltString::generate(&mut OsRng);
    
    let argon2 = Argon2::default();
    let hash = argon2.hash_password(password.as_bytes(), &salt).unwrap().to_string();
    
    println!("Hash: {}", hash);
    
    let parsed_hash = PasswordHash::new(&hash).unwrap();
    let result = argon2.verify_password(password.as_bytes(), &parsed_hash);
    
    if result.is_ok() {
        println!("SUCCESS: Verified!");
    } else {
        println!("FAILURE: Verification failed! {:?}", result.err());
    }
}
