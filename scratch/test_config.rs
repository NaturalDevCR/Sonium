use sonium_common::config::ServerConfig;

fn main() {
    let toml_str = r#"
[server]
bind         = "0.0.0.0"
stream_port  = 1710
control_port = 1711
mdns         = true

[server.audio]
buffer_ms         = 200
chunk_ms          = 10
output_prefill_ms = 0

[server.auto_buffer]
enabled       = false
min_ms        = 20
max_ms        = 3000
step_up_ms    = 120
step_down_ms  = 40
cooldown_ms   = 8000

[server.transport]
mode     = "tcp"
udp_port = 0

[[streams]]
id     = "default"
source = "-"
"#;

    match toml::from_str::<ServerConfig>(toml_str) {
        Ok(cfg) => println!("Success: {:?}", cfg),
        Err(e) => println!("Error: {}", e),
    }
}
