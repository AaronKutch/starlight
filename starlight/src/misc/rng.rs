use awint::awi::*;
use rand_xoshiro::{
    rand_core::{RngCore, SeedableRng},
    Xoshiro128StarStar,
};

/// A deterministic psuedo-random-number-generator. Is a wrapper around
/// `Xoshiro128StarStar` that buffers rng calls down to the bit level
#[derive(Debug)]
pub struct StarRng {
    rng: Xoshiro128StarStar,
    buf: inlawi_ty!(64),
    // invariant: `used < buf.bw()` and indicates the number of bits used out of `buf`
    used: u8,
}

macro_rules! next {
    ($($name:ident $x:ident $from:ident $to:ident),*,) => {
        $(
            pub fn $name(&mut self) -> $x {
                let mut res = InlAwi::$from(0);
                let mut processed = 0;
                loop {
                    let remaining_in_buf = usize::from(Self::BW_U8.wrapping_sub(self.used));
                    let remaining = res.bw().wrapping_sub(processed);
                    if remaining == 0 {
                        break
                    }
                    if remaining < remaining_in_buf {
                        res.field(
                            processed,
                            &self.buf,
                            usize::from(self.used),
                            remaining
                        ).unwrap();
                        self.used = self.used.wrapping_add(remaining as u8);
                        break
                    } else {
                        res.field(
                            processed,
                            &self.buf,
                            usize::from(self.used),
                            remaining_in_buf
                        ).unwrap();
                        processed = processed.wrapping_add(remaining_in_buf);
                        self.buf = InlAwi::from_u64(self.rng.next_u64());
                        self.used = 0;
                    }
                }
                res.$to()
            }
        )*
    };
}

impl StarRng {
    const BW_U8: u8 = 64;

    next!(
        next_u8 u8 from_u8 to_u8,
        next_u16 u16 from_u16 to_u16,
        next_u32 u32 from_u32 to_u32,
        next_u64 u64 from_u64 to_u64,
        next_u128 u128 from_u128 to_u128,
    );

    // note: do not implement `next_usize`, if it exists then there will be
    // arch-dependent rng code in a lot of places

    pub fn new(seed: u64) -> Self {
        let mut rng = Xoshiro128StarStar::seed_from_u64(seed);
        let buf = InlAwi::from_u64(rng.next_u64());
        Self { rng, buf, used: 0 }
    }

    pub fn next_bool(&mut self) -> bool {
        let res = self.buf.get(usize::from(self.used)).unwrap();
        self.used += 1;
        if self.used >= Self::BW_U8 {
            self.buf = InlAwi::from_u64(self.rng.next_u64());
            self.used = 0;
        }
        res
    }

    /// Assigns random value to `bits`
    pub fn next_bits(&mut self, bits: &mut Bits) {
        let mut processed = 0;
        loop {
            let remaining_in_buf = usize::from(Self::BW_U8.wrapping_sub(self.used));
            let remaining = bits.bw().wrapping_sub(processed);
            if remaining == 0 {
                break
            }
            if remaining < remaining_in_buf {
                bits.field(processed, &self.buf, usize::from(self.used), remaining)
                    .unwrap();
                self.used = self.used.wrapping_add(remaining as u8);
                break
            } else {
                bits.field(
                    processed,
                    &self.buf,
                    usize::from(self.used),
                    remaining_in_buf,
                )
                .unwrap();
                processed = processed.wrapping_add(remaining_in_buf);
                self.buf = InlAwi::from_u64(self.rng.next_u64());
                self.used = 0;
            }
        }
    }
}

impl RngCore for StarRng {
    fn next_u32(&mut self) -> u32 {
        self.next_u32()
    }

    fn next_u64(&mut self) -> u64 {
        self.next_u64()
    }

    fn fill_bytes(&mut self, dest: &mut [u8]) {
        // TODO make faster
        for byte in dest {
            *byte = self.next_u8();
        }
    }

    fn try_fill_bytes(&mut self, dest: &mut [u8]) -> Result<(), rand_xoshiro::rand_core::Error> {
        for byte in dest {
            *byte = self.next_u8();
        }
        Ok(())
    }
}

impl SeedableRng for StarRng {
    type Seed = [u8; 8];

    fn from_seed(seed: Self::Seed) -> Self {
        Self::new(u64::from_le_bytes(seed))
    }
}
