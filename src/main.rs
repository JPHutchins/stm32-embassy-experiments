#![no_std]
#![no_main]

mod button;
mod queue;
mod sampler;
mod timer;

use app::{AdcReport, App, Board, Machine, Post};
use button::{ButtonEdges, HwButton};
use embassy_executor::Spawner;
use embassy_stm32::bind_interrupts;
use embassy_stm32::gpio::Output;
use embassy_stm32::peripherals::{TIM3, TIM4};
use embassy_stm32::time::Hertz;
use embassy_time::Duration;
use queue::{CentralQueue, EVENTS};
use sampler::{AdcOverruns, CompletedHalves, HwSampler};
use timer::{BlinkTick, DebounceSettled, HwTimer, Posts};
use {defmt_rtt as _, panic_probe as _};

bind_interrupts!(struct Irqs {
    TIM3 => Posts<TIM3, BlinkTick>;
    TIM4 => Posts<TIM4, DebounceSettled>;
    EXTI0 => ButtonEdges;
    DMA2_STREAM0 => CompletedHalves;
    ADC => AdcOverruns;
});

const BLINK_PERIOD: Duration = Duration::from_millis(500);
const DEBOUNCE_PERIOD: Duration = Duration::from_millis(20);
const SCAN_RATE: Hertz = Hertz(2000);
const FRAMES_PER_HALF: usize = 1000;
const SAMPLES_PER_HALF: usize = FRAMES_PER_HALF * app::SEQUENCE.len();

type Event = app::Event<app::Lease<'static, SAMPLES_PER_HALF>>;

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    use embassy_stm32::gpio::{Level, Speed};

    let p = embassy_stm32::init(crystal_clocked());
    super_loop(
        EVENTS.receiver(),
        logged(App::<Disc1>::start(
            Output::new(p.PD12, Level::Low, Speed::Low),
            HwTimer::periodic(p.TIM3, BLINK_PERIOD, Irqs),
            HwButton::new(p.PA0, p.EXTI0, Irqs),
            HwTimer::one_shot(p.TIM4, DEBOUNCE_PERIOD, Irqs),
            CentralQueue,
            HwSampler::new(p.ADC1, p.DMA2_CH0, p.TIM2, SCAN_RATE, Irqs),
            Report,
        )),
    )
    .await;
}

fn crystal_clocked() -> embassy_stm32::Config {
    use embassy_stm32::rcc::{
        AHBPrescaler, APBPrescaler, Hse, HseMode, Pll, PllMul, PllPDiv, PllPreDiv, PllQDiv,
        PllSource, Sysclk,
    };

    let mut config = embassy_stm32::Config::default();
    config.rcc.hse = Some(Hse {
        freq: Hertz(8_000_000),
        mode: HseMode::Oscillator,
    });
    config.rcc.pll_src = PllSource::HSE;
    config.rcc.pll = Some(Pll {
        prediv: PllPreDiv::DIV4,
        mul: PllMul::MUL168,
        divp: Some(PllPDiv::DIV2),
        divq: Some(PllQDiv::DIV7),
        divr: None,
    });
    config.rcc.ahb_pre = AHBPrescaler::DIV1;
    config.rcc.apb1_pre = APBPrescaler::DIV4;
    config.rcc.apb2_pre = APBPrescaler::DIV2;
    config.rcc.sys = Sysclk::PLL1_P;
    config
}

async fn super_loop<M>(events: impl futures_util::Stream<Item = M::Event>, machine: M) -> M
where
    M: Machine + defmt::Format,
{
    use core::future::ready;
    use futures_util::StreamExt;

    events
        .fold(machine, |machine, event| ready(logged(machine.run(event))))
        .await
}

fn logged<T: defmt::Format>(value: T) -> T {
    defmt::debug!("{}", value);
    value
}

struct Disc1;

impl Board for Disc1 {
    type Led = Output<'static>;
    type BlinkTimer = HwTimer<TIM3, BlinkTick>;
    type Button = HwButton;
    type DebounceTimer = HwTimer<TIM4, DebounceSettled>;
    type Presses = CentralQueue;
    type Sampler = HwSampler;
    type Reports = Report;
}

struct Report;

impl Post<AdcReport> for Report {
    fn posted(self, report: AdcReport) -> Self {
        match report {
            AdcReport::Measured(measurement) => defmt::info!("{}", measurement),
            AdcReport::Overran => defmt::warn!("ADC overrun: sampling stopped"),
        }
        self
    }
}
