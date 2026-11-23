//! Integer-nanosecond progress without grouping whole CPU cycles.
#[derive(Default)]
pub(crate) struct CpuProgress {
    remainder: u64,
    mips: u32,
}
impl CpuProgress {
    pub(crate) fn advance(&mut self, cycles: u64, mips: u32) -> Result<u64, &'static str> {
        if mips == 0 {
            return Err("zero CPU pacing rate");
        }
        let remainder = if self.mips == 0 || self.mips == mips {
            u128::from(self.remainder)
        } else {
            // Preserve the sub-nanosecond phase across a rate change, rounded
            // down by less than one unit of the new fractional denominator.
            u128::from(self.remainder) * u128::from(mips) / u128::from(self.mips)
        };
        let ticks = u128::from(cycles) * 1000 + remainder;
        let ns = u64::try_from(ticks / u128::from(mips))
            .map_err(|_| "CPU progress interval overflow")?;
        self.remainder = (ticks % u128::from(mips)) as u64;
        self.mips = mips;
        Ok(ns)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn every_fixed_rate_preserves_phase_across_split_advances() {
        for mips in 1..=1024 {
            let mut clock = CpuProgress::default();
            let (mut cycles, mut ns) = (0u64, 0u64);
            for step in 0..64 {
                let delta = step * 17 % 23 + 1;
                cycles += delta;
                ns += clock.advance(delta, mips).unwrap();
                assert_eq!(u128::from(ns), u128::from(cycles) * 1000 / u128::from(mips));
            }
        }
    }
    #[test]
    fn one_cycle_makes_progress_without_waiting_for_an_eight_cycle_group() {
        let mut clock = CpuProgress::default();
        let intervals: Vec<_> = (0..8).map(|_| clock.advance(1, 64).unwrap()).collect();
        assert!(intervals.iter().all(|ns| [15, 16].contains(ns)));
        assert_eq!(intervals.iter().sum::<u64>(), 125);
    }
    #[test]
    fn rate_changes_keep_the_fraction_with_less_than_one_nanosecond_loss() {
        for first in 1..=100u32 {
            for second in 1..=100u32 {
                let mut clock = CpuProgress::default();
                let actual = clock.advance(11, first).unwrap() + clock.advance(23, second).unwrap();
                let exact = (11_000u128 * u128::from(second) + 23_000 * u128::from(first))
                    / (u128::from(first) * u128::from(second));
                assert!(exact >= u128::from(actual) && exact - u128::from(actual) <= 1);
            }
        }
    }
    #[test]
    fn invalid_intervals_do_not_consume_the_fractional_phase() {
        let mut clock = CpuProgress::default();
        assert_eq!(clock.advance(1, 64).unwrap(), 15);
        assert!(clock.advance(1, 0).is_err());
        assert!(clock.advance(u64::MAX, 64).is_err());
        assert_eq!(clock.advance(1, 64).unwrap(), 16);
        assert_eq!(
            CpuProgress::default().advance(u64::MAX, 1000).unwrap(),
            u64::MAX
        );
    }
}
