use cpal::traits::{DeviceTrait, HostTrait};
fn main() {
    let host = cpal::default_host();
    let device = host.default_output_device().unwrap();
    let config = cpal::StreamConfig {
        channels: 2,
        sample_rate: cpal::SampleRate(44100),
        buffer_size: cpal::BufferSize::Default,
    };
    println!("Trying 44100 with default buffer...");
    let result = device.build_output_stream(
        &config,
        |data: &mut [f32], _: &cpal::OutputCallbackInfo| {},
        |err| eprintln!("Error: {:?}", err),
        None,
    );
    println!("Result 44100: {:?}", result.is_ok());

    let config = cpal::StreamConfig {
        channels: 2,
        sample_rate: cpal::SampleRate(48000),
        buffer_size: cpal::BufferSize::Default,
    };
    println!("Trying 48000 with default buffer...");
    let result = device.build_output_stream(
        &config,
        |data: &mut [f32], _: &cpal::OutputCallbackInfo| {},
        |err| eprintln!("Error: {:?}", err),
        None,
    );
    println!("Result 48000: {:?}", result.is_ok());
}
