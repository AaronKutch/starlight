use starlight::{route::Router, Corresponder, Epoch, Error, In, Out, SuspendedEpoch};
use testcrate::targets::FabricTargetInterface;

struct SimpleCopyProgramInterface {
    input: In<1>,
    output: Out<1>,
    input2: In<1>,
}

impl SimpleCopyProgramInterface {
    pub fn definition() -> Self {
        let input = In::opaque();
        let output = Out::from_bits(&input).unwrap();
        Self {
            input,
            output,
            input2: In::opaque(),
        }
    }

    pub fn program() -> (Self, SuspendedEpoch) {
        let epoch = Epoch::new();
        let res = Self::definition();
        epoch.optimize().unwrap();
        (res, epoch.suspend())
    }
}

#[test]
fn route_errors_simple() {
    let (target, target_configurator, target_epoch) = FabricTargetInterface::target((2, 2));
    let (program, program_epoch) = SimpleCopyProgramInterface::program();

    let (third_interface, third_configurator, third_epoch) = FabricTargetInterface::target((2, 2));

    assert!(matches!(
        Router::new(&target_epoch, &third_configurator, &program_epoch),
        Err(Error::ConfigurationNotFound(_))
    ));
    assert!(matches!(
        Router::new(&program_epoch, &target_configurator, &target_epoch),
        Err(Error::ConfigurationNotFound(_))
    ));

    let mut router = Router::new(&target_epoch, &target_configurator, &program_epoch).unwrap();

    let mut corresponder = Corresponder::new();
    // create a single element correspondence
    corresponder
        .correspond_lazy(&program.input, &program.input)
        .unwrap();
    assert!(matches!(
        router.route(&corresponder),
        Err(Error::CorrespondenceWithoutTarget(_))
    ));

    let mut corresponder = Corresponder::new();
    // create a single element correspondence
    corresponder
        .correspond_eval(&program.output, &program.output)
        .unwrap();
    assert!(matches!(
        router.route(&corresponder),
        Err(Error::CorrespondenceWithoutTarget(_))
    ));

    let mut corresponder = Corresponder::new();
    // create a single element target correspondence
    corresponder
        .correspond_lazy(&target.inputs[0], &target.inputs[0])
        .unwrap();
    assert!(matches!(
        router.route(&corresponder),
        Err(Error::CorrespondenceWithoutProgram(_))
    ));

    let mut corresponder = Corresponder::new();
    // create a double program
    corresponder
        .correspond_lazy(&program.input, &program.input2)
        .unwrap();
    assert!(matches!(
        router.route(&corresponder),
        Err(Error::CorrespondenceDoubleProgram(_, _))
    ));

    let mut corresponder = Corresponder::new();
    // create correspondence with unrelated thing
    corresponder
        .correspond_lazy(&program.input, &third_interface.inputs[0])
        .unwrap();
    assert!(matches!(
        router.route(&corresponder),
        Err(Error::CorrespondenceNotFoundInEpoch(_))
    ));

    let mut corresponder = Corresponder::new();
    // create an unrelated correspondence
    corresponder
        .correspond_lazy(&third_interface.inputs[0], &third_interface.inputs[0])
        .unwrap();
    assert!(matches!(
        router.route(&corresponder),
        Err(Error::CorrespondenceNotFoundInEpoch(_))
    ));

    // create a valid correspondence
    let mut corresponder = Corresponder::new();
    corresponder
        .correspond_lazy(&program.input, &target.inputs[0])
        .unwrap();
    corresponder
        .correspond_eval(&program.output, &target.outputs[0])
        .unwrap();

    let target_epoch = target_epoch.resume();

    // checking configuration state invalidation

    assert!(matches!(
        router.config_target(),
        Err(Error::RoutingIsInvalid)
    ));
    assert!(matches!(
        router.get_config(&target.switch_grid[(0, 0)].configs[0]),
        Err(Error::RoutingIsInvalid)
    ));
    router.route(&corresponder).unwrap();
    router.config_target().unwrap();
    let _ = router
        .get_config(&target.switch_grid[(0, 0)].configs[0])
        .unwrap();
    router.clear_mappings();
    assert!(matches!(
        router.config_target(),
        Err(Error::RoutingIsInvalid)
    ));
    assert!(matches!(
        router.get_config(&target.switch_grid[(0, 0)].configs[0]),
        Err(Error::RoutingIsInvalid)
    ));

    let target_epoch = target_epoch.suspend();

    router.map_rnodes_from_corresponder(&corresponder).unwrap();
    router.route_without_remapping().unwrap();

    let third_epoch = third_epoch.resume();

    assert!(matches!(
        router.config_target(),
        Err(Error::NotInTargetEpoch)
    ));
    assert!(matches!(
        router.get_config(&target.switch_grid[(0, 0)].configs[0]),
        Err(Error::NotInTargetEpoch)
    ));

    drop(third_epoch);

    let target_epoch = target_epoch.resume();

    router.config_target().unwrap();
    let _ = router
        .get_config(&target.switch_grid[(0, 0)].configs[0])
        .unwrap();

    // actually check to make sure embeddings are not double applied or something
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

    drop(target_epoch);
    drop(program_epoch);
}
