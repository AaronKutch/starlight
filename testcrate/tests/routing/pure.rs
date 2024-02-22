//! pure routing with no combinatorics

use starlight::{route::Router, Corresponder, Epoch, In, Out, SuspendedEpoch};

use super::FabricTargetInterface;

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
fn route_pure() {
    let (target, target_configurator, target_epoch) = FabricTargetInterface::target((2, 2));
    let (program, program_epoch) = SimpleCopyProgramInterface::program();

    let input_i = 0;
    let output_i = 0;
    let mut corresponder = Corresponder::new();
    corresponder
        .correspond_lazy(&program.input, &target.inputs[input_i])
        .unwrap();
    corresponder
        .correspond_eval(&program.output, &target.outputs[output_i])
        .unwrap();

    let mut router = Router::new(
        &target_epoch,
        &target_configurator,
        &program_epoch,
        &corresponder,
    )
    .unwrap();

    router.route().unwrap();
}
