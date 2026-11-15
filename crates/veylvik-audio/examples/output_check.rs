//! Open the default output, play one second of silence and report the device and clock:
//! cargo run -p veylvik-audio --example output_check
use std::sync::Arc;
fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    for device in veylvik_audio::devices() {
        println!(
            "device: {} [{}] · {}",
            device.label, device.name, device.detail
        );
    }
    let player = veylvik_audio::Player::open(None)?;
    println!("in use: {}", player.description());
    let rate = player.rate();
    let pcm = Arc::new(veylvik_audio::Pcm {
        rate,
        start: 5.0,
        samples: vec![0.0; rate as usize * 2],
    });
    player.play(&pcm, 5.0);
    let started = std::time::Instant::now();
    std::thread::sleep(std::time::Duration::from_millis(500));
    let position = player.position().ok_or("not playing")?;
    println!(
        "after {:.2} s wall clock the audio clock is at {:.3} s past 5.0",
        started.elapsed().as_secs_f64(),
        position - 5.0
    );
    player.stop();
    Ok(())
}
