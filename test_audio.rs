use cpal::traits::{DeviceTrait, HostTrait};
fn main() {
    let host = cpal::default_host();
    let device = host.default_output_device().unwrap();
    println!("Default output device: {}", device.name().unwrap());
    let default_config = device.default_output_config().unwrap();
    println!("Default config: {:?}", default_config);
    for config in device.supported_output_configs().unwrap() {
        println!("Supported config: {:?}", config);
    }
}
