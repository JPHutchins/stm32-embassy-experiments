use crate::Event;
use crate::queue::post;
use app::{ButtonEvent, EdgeInput};
use embassy_stm32::exti::ExtiInput;
use embassy_stm32::gpio::ExtiPin;
use embassy_stm32::interrupt::typelevel::{Binding, Handler};
use embassy_stm32::mode::Blocking;
use embassy_stm32::peripherals::PA0;
use embassy_stm32::{Peri, exti, pac};
use embedded_hal::digital::PinState;

type ButtonPin = PA0;
type ButtonIrq = <<ButtonPin as ExtiPin>::ExtiChannel as exti::Channel>::IRQ;
const BUTTON_EXTI_LINE: usize = 0;

pub struct ButtonEdges;

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

pub struct HwButton(ExtiInput<'static, Blocking>);

impl HwButton {
    pub fn new(
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
