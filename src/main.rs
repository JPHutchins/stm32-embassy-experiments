#![no_std]
#![no_main]

use app::{App, BlinkEvent, Board, ButtonEvent, EdgeInput, Event, Machine, Post, Timer};
use core::marker::PhantomData;
use embassy_executor::Spawner;
use embassy_stm32::exti::ExtiInput;
use embassy_stm32::gpio::{ExtiPin, Output};
use embassy_stm32::interrupt::typelevel::{Binding, Handler};
use embassy_stm32::mode::Blocking;
use embassy_stm32::peripherals::{PA0, TIM3, TIM4};
use embassy_stm32::timer::CoreInstance;
use embassy_stm32::timer::low_level;
use embassy_stm32::{Peri, bind_interrupts, exti, pac};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::Channel;
use embassy_time::Duration;
use embedded_hal::digital::PinState;
use {defmt_rtt as _, panic_probe as _};

bind_interrupts!(struct Irqs {
    TIM3 => Posts<TIM3, BlinkTick>;
    TIM4 => Posts<TIM4, DebounceSettled>;
    EXTI0 => ButtonEdges;
});

const QUEUE_CAPACITY: usize = 8;
const GUARANTEED_IN_FLIGHT: usize = 2;
static EVENTS: Channel<CriticalSectionRawMutex, Event, QUEUE_CAPACITY> = Channel::new();

const BLINK_PERIOD: Duration = Duration::from_millis(500);
const DEBOUNCE_PERIOD: Duration = Duration::from_millis(20);

type ButtonPin = PA0;
type ButtonIrq = <<ButtonPin as ExtiPin>::ExtiChannel as exti::Channel>::IRQ;
const BUTTON_EXTI_LINE: usize = 0;

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    use embassy_stm32::gpio::{Level, Speed};

    let p = embassy_stm32::init(Default::default());
    super_loop(
        EVENTS.receiver(),
        logged(App::<Disc1>::start(
            Output::new(p.PD12, Level::Low, Speed::Low),
            HwTimer::periodic(p.TIM3, BLINK_PERIOD, Irqs),
            HwButton::new(p.PA0, p.EXTI0, Irqs),
            HwTimer::one_shot(p.TIM4, DEBOUNCE_PERIOD, Irqs),
            CentralQueue,
        )),
    )
    .await;
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
    defmt::info!("{}", value);
    value
}

fn post(event: Event) {
    use app::Delivery;
    use embassy_sync::channel::TrySendError;

    match (
        event.delivery(),
        EVENTS.len() < QUEUE_CAPACITY - GUARANTEED_IN_FLIGHT,
    ) {
        (Delivery::Droppable, false) => {}
        (Delivery::Droppable, true) | (Delivery::Guaranteed, _) => match EVENTS.try_send(event) {
            Ok(()) => {}
            Err(TrySendError::Full(event)) => {
                defmt::panic!("event queue full, lost guaranteed {}", event)
            }
        },
    }
}

struct Disc1;

impl Board for Disc1 {
    type Led = Output<'static>;
    type BlinkTimer = HwTimer<TIM3, BlinkTick>;
    type Button = HwButton;
    type DebounceTimer = HwTimer<TIM4, DebounceSettled>;
    type Events = CentralQueue;
}

struct CentralQueue;

impl Post<Event> for CentralQueue {
    fn posted(self, event: Event) -> Self {
        post(event);
        self
    }
}

trait Role {
    const EVENT: Event;
}

enum BlinkTick {}

impl Role for BlinkTick {
    const EVENT: Event = Event::Blink(BlinkEvent::Tick);
}

enum DebounceSettled {}

impl Role for DebounceSettled {
    const EVENT: Event = Event::Button(ButtonEvent::Settled);
}

struct Posts<T, R>(PhantomData<(T, R)>);

impl<T: CoreInstance, R: Role> Handler<T::UpdateInterrupt> for Posts<T, R> {
    unsafe fn on_interrupt() {
        unsafe { pac::timer::TimCore::from_ptr(T::regs()) }
            .sr()
            .write(|w| {
                w.0 = !0;
                w.set_uif(false);
            });
        post(R::EVENT);
    }
}

struct HwTimer<T: CoreInstance, R>(low_level::Timer<'static, T>, PhantomData<R>);

impl<T: CoreInstance, R: Role> HwTimer<T, R> {
    fn periodic(
        timer: Peri<'static, T>,
        period: Duration,
        _irq: impl Binding<T::UpdateInterrupt, Posts<T, R>>,
    ) -> Self {
        use embassy_stm32::interrupt::typelevel::Interrupt;
        use embassy_stm32::timer::low_level::RoundTo;

        let timer = low_level::Timer::new(timer);
        timer.set_period(period, RoundTo::Slower);
        timer.generate_update_event();
        timer.enable_update_interrupt(true);
        T::UpdateInterrupt::unpend();
        unsafe { T::UpdateInterrupt::enable() };
        Self(timer, PhantomData)
    }

    fn one_shot(
        timer: Peri<'static, T>,
        period: Duration,
        irq: impl Binding<T::UpdateInterrupt, Posts<T, R>>,
    ) -> Self {
        Self::periodic(timer, period, irq).one_pulse()
    }

    fn one_pulse(self) -> Self {
        self.0.regs_core().cr1().modify(|w| w.set_opm(true));
        self
    }
}

impl<T: CoreInstance, R> Timer for HwTimer<T, R> {
    fn started(self) -> Self {
        self.0.reset();
        self.0.clear_update_interrupt();
        self.0.start();
        self
    }

    fn stopped(self) -> Self {
        self.0.stop();
        self
    }
}

struct ButtonEdges;

impl Handler<ButtonIrq> for ButtonEdges {
    unsafe fn on_interrupt() {
        pac::EXTI
            .imr(0)
            .modify(|w| w.set_line(BUTTON_EXTI_LINE, false));
        pac::EXTI
            .pr(0)
            .write(|w| w.set_line(BUTTON_EXTI_LINE, true));
        post(Event::Button(ButtonEvent::Edge));
    }
}

struct HwButton(ExtiInput<'static, Blocking>);

impl HwButton {
    fn new(
        pin: Peri<'static, ButtonPin>,
        channel: Peri<'static, <ButtonPin as ExtiPin>::ExtiChannel>,
        _irq: impl Binding<ButtonIrq, ButtonEdges>,
    ) -> Self {
        use embassy_stm32::exti::{Channel, TriggerEdge};
        use embassy_stm32::gpio::Pull;

        defmt::assert_eq!(usize::from(channel.number()), BUTTON_EXTI_LINE);
        Self(ExtiInput::new_blocking(
            pin,
            channel,
            Pull::None,
            TriggerEdge::Any,
        ))
        .disarmed()
        .unpended()
    }

    fn unpended(self) -> Self {
        use embassy_stm32::interrupt::typelevel::Interrupt;

        ButtonIrq::unpend();
        self
    }
}

impl EdgeInput for HwButton {
    fn armed(mut self) -> Self {
        self.0.clear_pending();
        self.0.enable_interrupt();
        self
    }

    fn disarmed(mut self) -> Self {
        self.0.disable_interrupt();
        self.0.clear_pending();
        self
    }

    fn sampled(self) -> (PinState, Self) {
        (PinState::from(self.0.is_high()), self)
    }
}
