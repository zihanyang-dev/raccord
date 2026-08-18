#![forbid(unsafe_code)]

use core::num::NonZeroU64;

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct FrameIndex(u64);

impl FrameIndex {
    pub const ZERO: Self = Self(0);

    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    pub const fn value(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct FrameCount(u64);

impl FrameCount {
    pub const ZERO: Self = Self(0);

    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    pub const fn value(self) -> u64 {
        self.0
    }

    pub const fn is_zero(self) -> bool {
        self.0 == 0
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SampleIndex(u64);

impl SampleIndex {
    pub const ZERO: Self = Self(0);

    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    pub const fn value(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct Rational {
    numerator: u64,
    denominator: NonZeroU64,
}

impl Rational {
    pub fn new(numerator: u64, denominator: u64) -> Option<Self> {
        Some(Self {
            numerator,
            denominator: NonZeroU64::new(denominator)?,
        })
    }

    pub const fn numerator(self) -> u64 {
        self.numerator
    }

    pub const fn denominator(self) -> u64 {
        self.denominator.get()
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct FrameRate(Rational);

impl FrameRate {
    pub fn new(numerator: u64, denominator: u64) -> Option<Self> {
        Rational::new(numerator, denominator).map(Self)
    }

    pub const fn rational(self) -> Rational {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct FrameRange {
    start: FrameIndex,
    duration: FrameCount,
}

impl FrameRange {
    pub const fn new(start: FrameIndex, duration: FrameCount) -> Self {
        Self { start, duration }
    }

    pub const fn start(self) -> FrameIndex {
        self.start
    }

    pub const fn duration(self) -> FrameCount {
        self.duration
    }
}
