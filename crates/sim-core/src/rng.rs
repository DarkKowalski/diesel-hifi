//! A small explicitly seeded PRNG.
//!
//! Milestone 1 has no stochastic term, but SPEC section 6 requires that any
//! randomness be seeded explicitly and that stepping stay deterministic. The
//! generator is carried in simulation state and reset from [`crate::sim::ResetOptions::seed`]
//! so that later cycle-to-cycle variation cannot be added without a seed.
//!
//! PCG-XSH-RR 64/32. No dependencies, no global state, no allocation.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pcg32 {
    state: u64,
    increment: u64,
}

const MULTIPLIER: u64 = 6_364_136_223_846_793_005;
const DEFAULT_INCREMENT: u64 = 1_442_695_040_888_963_407;

impl Pcg32 {
    /// Deterministically seed the generator.
    pub fn seed_from(seed: u64) -> Self {
        let mut rng = Self {
            state: 0,
            increment: (DEFAULT_INCREMENT << 1) | 1,
        };
        rng.next_u32();
        rng.state = rng.state.wrapping_add(seed);
        rng.next_u32();
        rng
    }

    #[inline]
    pub fn next_u32(&mut self) -> u32 {
        let old = self.state;
        self.state = old.wrapping_mul(MULTIPLIER).wrapping_add(self.increment);
        let xorshifted = (((old >> 18) ^ old) >> 27) as u32;
        let rot = (old >> 59) as u32;
        xorshifted.rotate_right(rot)
    }

    /// Uniform in `[0, 1)`.
    #[inline]
    pub fn next_f64(&mut self) -> f64 {
        const SCALE: f64 = 1.0 / 4_294_967_296.0; // 1 / 2^32
        f64::from(self.next_u32()) * SCALE
    }
}
