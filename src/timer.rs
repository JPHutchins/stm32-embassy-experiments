use crate::Event;
use crate::queue::post;
use app::{BlinkEvent, ButtonEvent, Timer};
use core::marker::PhantomData;
use embassy_stm32::interrupt::typelevel::{Binding, Handler};
use embassy_stm32::timer::CoreInstance;
use embassy_stm32::timer::low_level;
use embassy_stm32::{Peri, pac};
use embassy_time::Duration;

pub trait Role {
    const EVENT: Event;
}

pub enum BlinkTick {}

impl Role for BlinkTick {
    const EVENT: Event = Event::Blink(BlinkEvent::Tick);
}

pub enum DebounceSettled {}

impl Role for DebounceSettled {
    const EVENT: Event = Event::Button(ButtonEvent::Settled);
}

pub struct Posts<T, R>(PhantomData<(T, R)>);

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

pub struct HwTimer<T: CoreInstance, R>(low_level::Timer<'static, T>, PhantomData<R>);

impl<T: CoreInstance, R: Role> HwTimer<T, R> {
    pub fn periodic(
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

    pub fn one_shot(
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
