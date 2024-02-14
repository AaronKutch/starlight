// pure routing with no combinatorics

use std::array;

use starlight::{
    self,
    awi::*,
    utils::{Ortho::*, OrthoArray},
    Drive, In, LazyAwi, Net, Out,
};

// TODO in another file test routing an example state machine over an island
// fabric with NAND static LUTs only, garbage, routing through inversion etc.

/// Each config selects between inputs from the orthogonal directions. The
/// simplest 2D switch that allows crossings needs N > 1.
#[derive(Debug)]
struct Switch<const N: usize> {
    inputs: OrthoArray<[Option<In<1>>; N]>,
    outputs: [Option<Out<1>>; N],
    configs: [LazyAwi; N],
}

impl<const N: usize> Switch<N> {
    pub fn definition() -> Self {
        let mut res = Self {
            inputs: OrthoArray::from_fn(|_| array::from_fn(|_| Some(In::opaque()))),
            outputs: array::from_fn(|_| None),
            configs: array::from_fn(|_| {
                LazyAwi::opaque(bw((N * 4).next_power_of_two().trailing_zeros() as usize))
            }),
        };
        // connect the inputs and outputs with nets
        for (i, output) in res.outputs.iter_mut().enumerate() {
            let mut net = Net::opaque(bw(1));
            for side in &res.inputs {
                for input in side {
                    net.push(input.as_ref().unwrap()).unwrap();
                }
            }
            *output = Some(Out::from_bits(&net).unwrap());
            net.drive(&res.configs[i]).unwrap();
        }
        res
    }

    // terminology: drive is one way, bridge is both ways
    pub fn bridge(&mut self, rhs: &mut Self, ortho: bool) {
        if ortho {
            for i in 0..N {
                rhs.inputs[Neg1][i].drive(&self.outputs[i]).unwrap();
                self.inputs[Pos1][i].drive(&rhs.outputs[i]).unwrap();
            }
        } else {
            for i in 0..N {
                rhs.inputs[Neg0][i].drive(&self.outputs[i]).unwrap();
                self.inputs[Pos0][i].drive(&rhs.outputs[i]).unwrap();
            }
        }
    }
}
