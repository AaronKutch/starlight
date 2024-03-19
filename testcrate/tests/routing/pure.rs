//! pure routing with no combinatorics

use starlight::{dag, delay, route::Router, Corresponder, Epoch, In, Out, SuspendedEpoch};
use testcrate::targets::FabricTargetInterface;
struct SimpleCopyProgramInterface {
    input: In<1>,
    output: Out<1>,
}

impl SimpleCopyProgramInterface {
    pub fn definition() -> Self {
        use dag::*;
        let input = In::opaque();
        let mut x = Awi::from_bits(&input);
        delay(&mut x, 1);
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

#[test]
fn route_empty() {
    let (_, target_configurator, target_epoch) = FabricTargetInterface::target((2, 2));
    let (_, program_epoch) = SimpleCopyProgramInterface::program();

    let corresponder = Corresponder::new();

    let mut router = Router::new(&target_epoch, &target_configurator, &program_epoch).unwrap();
    router.verify_integrity().unwrap();

    router.route(&corresponder).unwrap();
    router.verify_integrity().unwrap();

    let target_epoch = target_epoch.resume();

    router.config_target().unwrap();

    drop(target_epoch);
    drop(program_epoch);
}

#[test]
fn route_pure_single_small() {
    let (target, target_configurator, target_epoch) = FabricTargetInterface::target((3, 3));
    let (program, program_epoch) = SimpleCopyProgramInterface::program();
    let mut router = Router::new(&target_epoch, &target_configurator, &program_epoch).unwrap();
    let target_epoch = target_epoch.resume();

    // test every combination for this small case to catch direction sensitive edge
    // cases
    for input_i in 0..target.inputs.len() {
        for output_i in 0..target.outputs.len() {
            let mut corresponder = Corresponder::new();
            corresponder
                .correspond_lazy(&program.input, &target.inputs[input_i])
                .unwrap();
            corresponder
                .correspond_eval(&program.output, &target.outputs[output_i])
                .unwrap();

            router.verify_integrity().unwrap();
            router.route(&corresponder).unwrap();
            router.verify_integrity().unwrap();

            router.config_target().unwrap();

            corresponder
                .transpose_lazy(&program.input)
                .unwrap()
                .retro_bool_(true)
                .unwrap();
            assert!(corresponder
                .transpose_eval(&program.output)
                .unwrap()
                .eval_bool()
                .unwrap());
            corresponder
                .transpose_lazy(&program.input)
                .unwrap()
                .retro_bool_(false)
                .unwrap();
            assert!(!corresponder
                .transpose_eval(&program.output)
                .unwrap()
                .eval_bool()
                .unwrap());
        }
    }
    drop(target_epoch);
    drop(program_epoch);
}

// TODO when the more advanced general routing is done redo this
#[test]
fn route_pure_stats() {
    let (_, target_configurator, target_epoch) = FabricTargetInterface::target((2, 2));
    let (_, program_epoch) = SimpleCopyProgramInterface::program();
    let router = Router::new(&target_epoch, &target_configurator, &program_epoch).unwrap();
    assert_eq!(router.target_channeler().cnodes.len(), 30);
    assert_eq!(router.target_channeler().cedges.len(), 9);
}
