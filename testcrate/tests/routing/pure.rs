// pure routing with no combinatorics

use std::array;

use starlight::{
    self,
    awi::*,
    route::{Configurator, Router},
    utils::{Grid, Ortho::*, OrthoArray},
    Drive, Epoch, In, LazyAwi, Net, Out, SuspendedEpoch,
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
        // each output can be driven by any input
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

struct FabricTargetInterface {
    // row major order
    switch_grid: Grid<Switch<2>>,
    // exposed switch IO
    inputs: Vec<In<1>>,
    outputs: Vec<Out<1>>,
}

impl FabricTargetInterface {
    pub fn definition(len: (usize, usize)) -> Self {
        let mut switch_grid = Grid::new(len, |_| Switch::definition()).unwrap();
        switch_grid
            .for_each_orthogonal_pair_mut(|switch0, _, switch1, dir| switch0.bridge(switch1, dir));
        let mut inputs = vec![];
        let mut outputs = vec![];
        switch_grid.for_each_edge_mut(|switch, (i, j), ortho| {
            if let Some(output) = switch.outputs[0].take() {
                output.set_debug_name(format!("out.{ortho}.{i}.0")).unwrap();
                outputs.push(output);
            }
            if let Some(output) = switch.outputs[1].take() {
                output.set_debug_name(format!("out.{ortho}.{i}.1")).unwrap();
                outputs.push(output);
            }
            for (input_i, input) in switch.inputs[ortho].iter_mut().enumerate() {
                let input = input.take().unwrap();
                input
                    .set_debug_name(format!("in.{ortho}.({i}, {j}).{input_i}"))
                    .unwrap();
                inputs.push(input);
            }
        });
        // check that everything has been connected or moved to the common inputs and
        // outputs
        switch_grid.for_each(|switch, _| {
            for input_array in &switch.inputs {
                for input in input_array {
                    assert!(input.is_none());
                }
            }
        });
        Self {
            switch_grid,
            inputs,
            outputs,
        }
    }

    pub fn target(len: (usize, usize)) -> (Self, Configurator, SuspendedEpoch) {
        let epoch = Epoch::new();
        let res = Self::definition(len);
        epoch.optimize().unwrap();
        let mut target_configurator = Configurator::new();
        epoch.ensemble(|ensemble| {
            res.switch_grid.for_each(|switch, _| {
                for config in &switch.configs {
                    target_configurator
                        .make_configurable(ensemble, config)
                        .unwrap();
                }
            });
        });
        (res, target_configurator, epoch.suspend())
    }
}

struct SimpleCopyProgramInterface {
    input: In<1>,
    output: Out<1>,
}

impl SimpleCopyProgramInterface {
    pub fn definition() -> Self {
        let input = In::opaque();
        let output = Out::from_bits(&input).unwrap();
        Self { input, output }
    }

    pub fn program() -> (Self, SuspendedEpoch) {
        let epoch = Epoch::new();
        let res = Self::definition();
        epoch.optimize().unwrap();
        (res, epoch.suspend())
    }
}

// TODO corner cases

#[test]
fn pure_route() {
    let (target, target_configurator, target_epoch) = FabricTargetInterface::target((2, 2));
    let (program, program_epoch) = SimpleCopyProgramInterface::program();

    let mut router = Router::new(&target_epoch, &target_configurator, &program_epoch).unwrap();
    let input_i = 0;
    let output_i = 0;
    router
        .map_lazy(&program.input, &target.inputs[input_i])
        .unwrap();
    router
        .map_eval(&program.output, &target.outputs[output_i])
        .unwrap();
    router.route().unwrap();
}
