#![no_std]
#![no_main]

use defmt::info;
use embassy_executor::Spawner;
use embassy_stm32::gpio::{Level, Output, Speed};
use embassy_time::Timer;
use {defmt_rtt as _, panic_probe as _};

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    let mut green_led = Output::new(
        embassy_stm32::init(Default::default()).PD12,
        Level::Low,
        Speed::Low,
    );
    info!("STM32F407G-DISC1 up");

    loop {
        green_led.toggle();
        Timer::after_millis(500).await;
    }
}
