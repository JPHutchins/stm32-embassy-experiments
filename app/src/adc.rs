use crate::{Machine, Post, Sampler};

#[derive(Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum AdcEvent<S> {
    Start,
    Stop,
    Samples(S),
    Overrun,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Adc<A, M> {
    Sampling(Sampling<A, M>),
    Stopped(Stopped<A, M>),
}

impl<A, M> Machine for Adc<A, M>
where
    A: Sampler,
    M: Post<AdcReport>,
{
    type Event = AdcEvent<A::Samples>;

    fn run(self, event: Self::Event) -> Self {
        match (self, event) {
            (Self::Sampling(sampling), AdcEvent::Samples(samples)) => {
                Self::Sampling(sampling.measured(&samples))
            }
            (Self::Sampling(sampling), AdcEvent::Stop) => {
                Self::Stopped(sampling.stop(Cause::Requested))
            }
            (Self::Sampling(sampling), AdcEvent::Overrun) => {
                Self::Stopped(sampling.stop(Cause::Overran))
            }
            (Self::Sampling(sampling), AdcEvent::Start) => Self::Sampling(sampling),
            (Self::Stopped(stopped), AdcEvent::Start) => Self::Sampling(stopped.restart()),
            (Self::Stopped(stopped), AdcEvent::Overrun) => Self::Stopped(stopped.overran()),
            (Self::Stopped(stopped), AdcEvent::Stop | AdcEvent::Samples(_)) => {
                Self::Stopped(stopped)
            }
        }
    }
}

impl<A, M> Adc<A, M>
where
    A: Sampler,
    M: Post<AdcReport>,
{
    pub fn start(sampler: A, reports: M) -> Self {
        Self::Sampling(Sampling::start(sampler, reports))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Input {
    Vrefint,
    Temperature,
}

pub const SEQUENCE: [Input; 2] = [Input::Vrefint, Input::Temperature];
pub const DECIMATED_LEN: usize = 10;

const _: [usize; 2] = [Input::Vrefint.rank(), Input::Temperature.rank()];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct Measurement {
    vrefint_max: u16,
    temperature: [u16; DECIMATED_LEN],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum AdcReport {
    Measured(Measurement),
    Overran,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Sampling<A, M> {
    sampler: A,
    reports: M,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Stopped<A, M> {
    sampler: A,
    reports: M,
    cause: Cause,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Cause {
    Requested,
    Overran,
}

impl<A, M> Sampling<A, M>
where
    A: Sampler,
    M: Post<AdcReport>,
{
    fn start(sampler: A, reports: M) -> Self {
        Self {
            sampler: sampler.started(),
            reports,
        }
    }

    fn measured(self, samples: &[u16]) -> Self {
        Self {
            reports: match Measurement::of(samples) {
                Some(measurement) => self.reports.posted(AdcReport::Measured(measurement)),
                None => self.reports,
            },
            ..self
        }
    }

    fn stop(self, cause: Cause) -> Stopped<A, M> {
        Stopped {
            sampler: self.sampler.stopped(),
            reports: reported(self.reports, cause),
            cause,
        }
    }
}

impl<A, M> Stopped<A, M>
where
    A: Sampler,
    M: Post<AdcReport>,
{
    fn restart(self) -> Sampling<A, M> {
        Sampling::start(self.sampler, self.reports)
    }

    fn overran(self) -> Self {
        Self {
            reports: reported(self.reports, Cause::Overran),
            cause: Cause::Overran,
            ..self
        }
    }
}

fn reported<M: Post<AdcReport>>(reports: M, cause: Cause) -> M {
    match cause {
        Cause::Overran => reports.posted(AdcReport::Overran),
        Cause::Requested => reports,
    }
}

impl Measurement {
    fn of(samples: &[u16]) -> Option<Self> {
        channel(samples, Input::Vrefint)
            .max()
            .map(|vrefint_max| Self {
                vrefint_max,
                temperature: decimated(samples, Input::Temperature),
            })
    }
}

impl Input {
    const fn rank(self) -> usize {
        rank_in(&SEQUENCE, self)
    }
}

const fn rank_in(sequence: &[Input], input: Input) -> usize {
    match sequence {
        [first, rest @ ..] => match (first, input) {
            (Input::Vrefint, Input::Vrefint) | (Input::Temperature, Input::Temperature) => 0,
            (Input::Vrefint, Input::Temperature) | (Input::Temperature, Input::Vrefint) => {
                1 + rank_in(rest, input)
            }
        },
        [] => panic!("every Input must appear in SEQUENCE"),
    }
}

fn channel(samples: &[u16], input: Input) -> impl Iterator<Item = u16> + '_ {
    samples
        .iter()
        .copied()
        .skip(input.rank())
        .step_by(SEQUENCE.len())
}

fn decimated(samples: &[u16], input: Input) -> [u16; DECIMATED_LEN] {
    core::array::from_fn(|block| mean(channel(block_of(samples, block), input)))
}

fn block_of(samples: &[u16], block: usize) -> &[u16] {
    samples
        .chunks_exact((samples.len() / SEQUENCE.len() / DECIMATED_LEN).max(1) * SEQUENCE.len())
        .nth(block)
        .unwrap_or(&[])
}

fn mean(samples: impl Iterator<Item = u16>) -> u16 {
    let (sum, count) = samples.fold((0u32, 0u32), |(sum, count), sample| {
        (sum + u32::from(sample), count + 1)
    });
    u16::try_from(sum.checked_div(count).unwrap_or(0)).unwrap_or(u16::MAX)
}

#[cfg(feature = "defmt")]
impl<A, M> defmt::Format for Adc<A, M> {
    fn format(&self, f: defmt::Formatter) {
        match self {
            Self::Sampling(_) => defmt::write!(f, "Sampling"),
            Self::Stopped(stopped) => defmt::write!(f, "Stopped({})", stopped.cause),
        }
    }
}

#[cfg(test)]
mod tests {
    extern crate std;

    use super::{
        Adc, AdcEvent, AdcReport, Cause, DECIMATED_LEN, Input, Measurement, SEQUENCE, Sampling,
        Stopped, channel, decimated,
    };
    use crate::mock::{MockReports, MockSampler};
    use crate::{Completion, DoubleBuffer, Half, Machine, Sampler};
    use std::vec::Vec;

    fn sampling(posted: &[AdcReport]) -> Adc<MockSampler, MockReports> {
        Adc::Sampling(Sampling {
            sampler: MockSampler::Started,
            reports: MockReports::from(posted),
        })
    }

    fn stopped(cause: Cause, posted: &[AdcReport]) -> Adc<MockSampler, MockReports> {
        Adc::Stopped(Stopped {
            sampler: MockSampler::Stopped,
            reports: MockReports::from(posted),
            cause,
        })
    }

    fn interleaved(frames: impl Iterator<Item = (u16, u16)>) -> Vec<u16> {
        frames
            .flat_map(|(vrefint, temperature)| [vrefint, temperature])
            .collect()
    }

    #[test]
    fn each_input_is_ranked_by_its_index_in_the_scan_sequence() {
        SEQUENCE
            .into_iter()
            .enumerate()
            .for_each(|(index, input)| assert_eq!(input.rank(), index, "{input:?}"));
    }

    #[test]
    fn a_channel_is_a_strided_view_of_the_interleaved_buffer() {
        let samples = interleaved([(1, 10), (2, 20), (3, 30)].into_iter());
        assert_eq!(
            channel(&samples, Input::Vrefint).collect::<Vec<_>>(),
            [1, 2, 3]
        );
        assert_eq!(
            channel(&samples, Input::Temperature).collect::<Vec<_>>(),
            [10, 20, 30]
        );
    }

    #[test]
    fn decimation_averages_equal_blocks_of_one_channel() {
        let samples = interleaved(
            (0..u16::try_from(2 * DECIMATED_LEN).expect("test length fits u16"))
                .map(|i| (5000, 100 * i)),
        );
        assert_eq!(
            decimated(&samples, Input::Temperature),
            core::array::from_fn(|block| {
                u16::try_from(200 * block + 50).expect("block mean fits u16")
            })
        );
    }

    #[test]
    fn a_measurement_is_the_vrefint_max_and_the_decimated_temperature() {
        let samples = interleaved([(3, 10), (7, 20), (5, 30)].into_iter());
        assert_eq!(
            Measurement::of(&samples),
            Some(Measurement {
                vrefint_max: 7,
                temperature: core::array::from_fn(|block| [10, 20, 30]
                    .get(block)
                    .copied()
                    .unwrap_or(0)),
            })
        );
        assert_eq!(Measurement::of(&[]), None);
    }

    #[test]
    fn start_starts_the_sampler() {
        assert_eq!(
            Adc::start(MockSampler::Stopped, MockReports::default()),
            sampling(&[])
        );
    }

    #[test]
    fn every_state_event_pair_drives_the_sampler_as_specified() {
        let measured = Measurement::of(&[3, 10, 1, 20]).expect("non-empty buffer measures");
        [
            (
                sampling(&[]),
                AdcEvent::Samples(Vec::from([3, 10, 1, 20])),
                sampling(&[AdcReport::Measured(measured)]),
            ),
            (sampling(&[]), AdcEvent::Samples(Vec::new()), sampling(&[])),
            (sampling(&[]), AdcEvent::Start, sampling(&[])),
            (
                sampling(&[]),
                AdcEvent::Stop,
                stopped(Cause::Requested, &[]),
            ),
            (
                sampling(&[]),
                AdcEvent::Overrun,
                stopped(Cause::Overran, &[AdcReport::Overran]),
            ),
            (
                stopped(Cause::Requested, &[]),
                AdcEvent::Start,
                sampling(&[]),
            ),
            (stopped(Cause::Overran, &[]), AdcEvent::Start, sampling(&[])),
            (
                stopped(Cause::Requested, &[]),
                AdcEvent::Stop,
                stopped(Cause::Requested, &[]),
            ),
            (
                stopped(Cause::Requested, &[]),
                AdcEvent::Samples(Vec::from([1, 2])),
                stopped(Cause::Requested, &[]),
            ),
            (
                stopped(Cause::Overran, &[]),
                AdcEvent::Samples(Vec::from([1, 2])),
                stopped(Cause::Overran, &[]),
            ),
            (
                stopped(Cause::Requested, &[]),
                AdcEvent::Overrun,
                stopped(Cause::Overran, &[AdcReport::Overran]),
            ),
            (
                stopped(Cause::Overran, &[]),
                AdcEvent::Overrun,
                stopped(Cause::Overran, &[AdcReport::Overran]),
            ),
        ]
        .into_iter()
        .for_each(|(state, event, expected)| assert_eq!(state.run(event), expected));
    }

    struct LeasingSampler;

    impl Sampler for LeasingSampler {
        type Samples = crate::Lease<'static, 2>;

        fn started(self) -> Self {
            self
        }

        fn stopped(self) -> Self {
            self
        }
    }

    static BUFFER: DoubleBuffer<2> = DoubleBuffer::new();

    #[test]
    fn handling_a_samples_event_releases_its_buffer_half() {
        match BUFFER.completed(Half::First) {
            Completion::Ready(lease) => {
                assert!(!BUFFER.idle());
                let _measured = Adc::start(LeasingSampler, MockReports::default())
                    .run(AdcEvent::Samples(lease));
                assert!(BUFFER.idle());
            }
            Completion::Overrun => panic!("a fresh buffer cannot overrun"),
        }
    }
}
