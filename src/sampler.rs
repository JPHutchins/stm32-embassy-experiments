use crate::queue::post;
use crate::{Event, FRAMES_PER_HALF, SAMPLES_PER_HALF};
use app::{AdcEvent, DoubleBuffer, Half, Lease, SEQUENCE, Sampler};
use embassy_stm32::dma::ChannelInstance;
use embassy_stm32::interrupt::typelevel::{Binding, Handler};
use embassy_stm32::peripherals::{ADC1, DMA2_CH0, TIM2};
use embassy_stm32::time::Hertz;
use embassy_stm32::timer::low_level;
use embassy_stm32::{Peri, pac};

static SAMPLES: DoubleBuffer<SAMPLES_PER_HALF> = DoubleBuffer::new();

const _: () = assert!(FRAMES_PER_HALF.is_multiple_of(app::DECIMATED_LEN));

type SampleDma = DMA2_CH0;
type SampleIrq = <SampleDma as ChannelInstance>::Interrupt;
type AdcIrq = <ADC1 as embassy_stm32::adc::Instance>::Interrupt;

trait AdcStream {
    const CONTROLLER: pac::dma::Dma;
    const STREAM: usize;
    const REQUEST: u8;
    const FLAG_REGISTER: usize = Self::STREAM / 4;
    const FLAG_INDEX: usize = Self::STREAM % 4;
}

impl AdcStream for DMA2_CH0 {
    const CONTROLLER: pac::dma::Dma = pac::DMA2;
    const STREAM: usize = 0;
    const REQUEST: u8 = 0;
}

const FLAG_INDEX: usize = <SampleDma as AdcStream>::FLAG_INDEX;
const EXTSEL_TIM2_TRGO: u8 = 0b0110;
const DMA_TRANSFERS: u16 = {
    assert!(DoubleBuffer::<SAMPLES_PER_HALF>::LEN <= u16::MAX as usize);
    DoubleBuffer::<SAMPLES_PER_HALF>::LEN as u16
};
const SCAN_LENGTH_MINUS_ONE: u8 = {
    assert!(!SEQUENCE.is_empty() && SEQUENCE.len() <= 6);
    (SEQUENCE.len() - 1) as u8
};

pub struct HwSampler {
    _adc: embassy_stm32::adc::Adc<'static, ADC1>,
    _dma: Peri<'static, SampleDma>,
    trigger: low_level::Timer<'static, TIM2>,
}

impl HwSampler {
    pub fn new(
        adc: Peri<'static, ADC1>,
        dma: Peri<'static, SampleDma>,
        trigger: Peri<'static, TIM2>,
        scan_rate: Hertz,
        _irqs: impl Binding<SampleIrq, CompletedHalves> + Binding<AdcIrq, AdcOverruns>,
    ) -> Self {
        defmt::assert!(
            (0x2000_0000..0x2002_0000).contains(&SAMPLES.dma_target().addr()),
            "DMA2 cannot reach the sample buffer outside SRAM1/SRAM2"
        );
        Self {
            _adc: scanning(embassy_stm32::adc::Adc::new(adc)),
            _dma: dma,
            trigger: trigger_at(low_level::Timer::new(trigger), scan_rate),
        }
    }
}

fn scanning(adc: embassy_stm32::adc::Adc<'static, ADC1>) -> embassy_stm32::adc::Adc<'static, ADC1> {
    use embassy_stm32::interrupt::typelevel::Interrupt;

    pac::ADC1.cr1().modify(|w| w.set_scan(true));
    pac::ADC1.sqr1().modify(|w| w.set_l(SCAN_LENGTH_MINUS_ONE));
    SEQUENCE
        .into_iter()
        .enumerate()
        .for_each(|(rank, input)| sequenced(rank, hw_channel(&adc, input)));
    AdcIrq::unpend();
    unsafe { AdcIrq::enable() };
    adc
}

fn hw_channel(adc: &embassy_stm32::adc::Adc<'static, ADC1>, input: app::Input) -> u8 {
    use app::Input;
    use embassy_stm32::adc::AdcChannel;

    match input {
        Input::Vrefint => AdcChannel::<ADC1>::degrade_adc(adc.enable_vrefint()).get_hw_channel(),
        Input::Temperature => {
            AdcChannel::<ADC1>::degrade_adc(adc.enable_temperature()).get_hw_channel()
        }
    }
}

fn sequenced(rank: usize, channel: u8) {
    use embassy_stm32::pac::adc::vals::SampleTime;

    pac::ADC1.sqr3().modify(|w| w.set_sq(rank, channel));
    match channel.checked_sub(10) {
        Some(high) => pac::ADC1
            .smpr1()
            .modify(|w| w.set_smp(usize::from(high), SampleTime::CYCLES480)),
        None => pac::ADC1
            .smpr2()
            .modify(|w| w.set_smp(usize::from(channel), SampleTime::CYCLES480)),
    }
}

fn trigger_at(
    timer: low_level::Timer<'static, TIM2>,
    scan_rate: Hertz,
) -> low_level::Timer<'static, TIM2> {
    use embassy_stm32::pac::timer::vals::Mms;
    use embassy_stm32::timer::low_level::RoundTo;

    timer.set_frequency(scan_rate, RoundTo::Slower);
    timer.regs_basic().cr2().modify(|w| w.set_mms(Mms::UPDATE));
    timer.generate_update_event();
    defmt::info!(
        "ADC scan trigger: {} Hz from a {} Hz timer clock",
        timer.get_frequency().0,
        timer.get_clock_frequency().0
    );
    timer
}

impl Sampler for HwSampler {
    type Samples = Lease<'static, SAMPLES_PER_HALF>;

    fn started(self) -> Self {
        defmt::assert!(
            SAMPLES.idle(),
            "restarting the ADC while a sample buffer is still leased"
        );
        start_conversions();
        self.trigger.reset();
        self.trigger.start();
        self
    }

    fn stopped(self) -> Self {
        self.trigger.stop();
        stop_conversions();
        self
    }
}

pub struct CompletedHalves;

impl Handler<SampleIrq> for CompletedHalves {
    unsafe fn on_interrupt() {
        completed_halves(
            <SampleDma as AdcStream>::CONTROLLER
                .isr(<SampleDma as AdcStream>::FLAG_REGISTER)
                .read(),
        );
    }
}

pub struct AdcOverruns;

impl Handler<AdcIrq> for AdcOverruns {
    unsafe fn on_interrupt() {
        if pac::ADC1.sr().read().ovr() {
            overran();
        }
    }
}

fn completed_halves(flags: pac::dma::regs::Ixr) {
    clear_stream_flags();
    match (
        flags.teif(FLAG_INDEX),
        flags.htif(FLAG_INDEX),
        flags.tcif(FLAG_INDEX),
    ) {
        (true, _, _) => defmt::panic!(
            "DMA stream {} transfer error",
            <SampleDma as AdcStream>::STREAM
        ),
        (false, true, true) => overran(),
        (false, true, false) => completed(Half::First),
        (false, false, true) => completed(Half::Second),
        (false, false, false) => {}
    }
}

fn completed(half: Half) {
    use app::Completion;

    match SAMPLES.completed(half) {
        Completion::Ready(lease) => post(Event::Adc(AdcEvent::Samples(lease))),
        Completion::Overrun => overran(),
    }
}

fn overran() {
    stop_conversions();
    post(Event::Adc(AdcEvent::Overrun));
}

fn stream() -> pac::dma::St {
    <SampleDma as AdcStream>::CONTROLLER.st(<SampleDma as AdcStream>::STREAM)
}

fn start_conversions() {
    use embassy_stm32::pac::adc::vals::{Dds, Exten};
    use embassy_stm32::pac::dma::vals::{Dir, Pl, Size};

    stream()
        .par()
        .write_value(pac::ADC1.dr().as_ptr().addr() as u32);
    stream()
        .m0ar()
        .write_value(SAMPLES.dma_target().addr() as u32);
    stream().ndtr().write(|w| w.set_ndt(DMA_TRANSFERS));
    stream().cr().write(|w| {
        w.set_chsel(<SampleDma as AdcStream>::REQUEST);
        w.set_dir(Dir::PERIPHERAL_TO_MEMORY);
        w.set_psize(Size::BITS16);
        w.set_msize(Size::BITS16);
        w.set_minc(true);
        w.set_circ(true);
        w.set_pl(Pl::HIGH);
        w.set_htie(true);
        w.set_tcie(true);
        w.set_teie(true);
    });
    stream().cr().modify(|w| w.set_en(true));
    clear_adc_overrun();
    pac::ADC1.cr1().modify(|w| w.set_ovrie(true));
    pac::ADC1.cr2().modify(|w| w.set_dma(false));
    pac::ADC1.cr2().modify(|w| {
        w.set_cont(false);
        w.set_dma(true);
        w.set_dds(Dds::CONTINUOUS);
        w.set_extsel(EXTSEL_TIM2_TRGO);
        w.set_exten(Exten::RISING_EDGE);
    });
}

fn stop_conversions() {
    use embassy_stm32::interrupt::typelevel::Interrupt;
    use embassy_stm32::pac::adc::vals::Exten;

    pac::ADC1.cr1().modify(|w| w.set_ovrie(false));
    pac::ADC1.cr2().modify(|w| {
        w.set_exten(Exten::DISABLED);
        w.set_dma(false);
    });
    stream().cr().modify(|w| {
        w.set_en(false);
        w.set_htie(false);
        w.set_tcie(false);
        w.set_teie(false);
    });
    while stream().cr().read().en() {}
    clear_stream_flags();
    clear_adc_overrun();
    SampleIrq::unpend();
    AdcIrq::unpend();
}

fn clear_adc_overrun() {
    pac::ADC1.sr().write(|w| {
        w.0 = !0;
        w.set_ovr(false);
    });
}

fn clear_stream_flags() {
    <SampleDma as AdcStream>::CONTROLLER
        .ifcr(<SampleDma as AdcStream>::FLAG_REGISTER)
        .write(|w| {
            w.set_tcif(FLAG_INDEX, true);
            w.set_htif(FLAG_INDEX, true);
            w.set_teif(FLAG_INDEX, true);
            w.set_dmeif(FLAG_INDEX, true);
            w.set_feif(FLAG_INDEX, true);
        });
}
