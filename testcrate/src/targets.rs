//! This is here as a library so that local test binaries can use it

use std::{array, collections::HashMap};

use starlight::{
    awi::*,
    ensemble::{
        render::{RenderArena, RenderNodeKind},
        PExternal,
    },
    route::{Channeler, Configurator},
    triple_arena::{ptr_struct, OrdArena, Ptr},
    utils::{Grid, Ortho::*, OrthoArray, Render},
    Drive, Epoch, In, LazyAwi, Net, Out, SuspendedEpoch,
};

// TODO in another file test routing an example state machine over an island
// fabric with NAND static LUTs only, garbage, routing through inversion etc.

/// Each config selects between inputs from the orthogonal directions. The
/// simplest 2D switch that allows crossings needs N > 1.
#[derive(Debug)]
pub struct Switch<const N: usize> {
    pub inputs: OrthoArray<[Option<In<1>>; N]>,
    pub outputs: [Option<Out<1>>; N],
    pub configs: [LazyAwi; N],
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

pub struct FabricTargetInterface {
    // row major order
    pub switch_grid: Grid<Switch<2>>,
    // exposed switch IO
    pub inputs: Vec<In<1>>,
    pub outputs: Vec<Out<1>>,
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
        res.switch_grid.for_each(|switch, _| {
            for config in &switch.configs {
                target_configurator.configurable(config).unwrap();
            }
        });
        (res, target_configurator, epoch.suspend())
    }

    #[allow(unused)]
    pub fn to_rendered(&self, epoch: &SuspendedEpoch) -> (Render, RenderArena) {
        const BLOCK_W: i32 = 256;
        const PAD: i32 = 32;
        let mut r = Render::new((
            BLOCK_W * (self.switch_grid.len().0 as i32),
            BLOCK_W * (self.switch_grid.len().1 as i32),
        ));

        // map the switch outputs and configuration bits to a grid of points
        let mut fixed = HashMap::<PExternal, (i32, i32)>::new();
        self.switch_grid.for_each(|switch, (i, j)| {
            let mut corner = (i as i32 * BLOCK_W, j as i32 * BLOCK_W);
            corner.0 += PAD;
            corner.1 += PAD;

            r.rects.push((
                corner.0,
                corner.1,
                BLOCK_W - (2 * PAD),
                BLOCK_W - (2 * PAD),
                Render::EIGENGRAU.to_owned(),
            ));

            let mut output_point = corner;
            for output in &switch.outputs {
                output_point.0 += PAD;
                output_point.1 += PAD;
                if let Some(output) = output.as_ref() {
                    fixed.insert(output.p_external(), output_point);
                }
            }

            let mut config_point = output_point;
            config_point.0 += PAD;
            for config in &switch.configs {
                config_point.1 += PAD;
                fixed.insert(config.p_external(), config_point);
            }
        });

        // draw points and lines between them
        for xy in fixed.values() {
            r.circles.push((*xy, 16, Render::COLORS[0].to_owned()));
        }

        let web = epoch.ensemble(|ensemble| ensemble.debug_web(fixed.clone()));
        for node in web.vals() {
            r.circles
                .push((node.position, 8, Render::COLORS[1].to_owned()));
            for edge in &node.incidents {
                let p_other = web.find_key(edge).unwrap();
                let edge = web.get_val(p_other).unwrap().position;
                r.lines
                    .push((node.position, edge, 4, Render::COLORS[1].to_owned()));
            }
        }

        (r, web)
    }
}

#[allow(unused)]
pub fn render_cnode_hierarchy<PBack: Ptr, PCEdge: Ptr>(
    r: &mut Render,
    web: &RenderArena,
    channeler: &Channeler<PBack, PCEdge>,
) {
    ptr_struct!(P0);
    struct HierarchyNode<PBack: Ptr> {
        position: (i32, i32),
        subnodes: usize,
        incidents: Vec<PBack>,
    }
    let mut levels = vec![];
    // get the first level of nodes
    let mut level = OrdArena::<P0, PBack, HierarchyNode<PBack>>::new();
    let cnodes = &channeler.cnodes;
    for (_, kind, node) in web {
        if let RenderNodeKind::Equiv(p_back) = kind {
            // remember that configurable bits are not included
            if let Some(p_cnode) = channeler.find_channeler_cnode(*p_back) {
                let cnode = cnodes.get_val(p_cnode).unwrap();
                assert_eq!(cnode.lvl, 0);
                let replaced = level
                    .insert(p_cnode, HierarchyNode {
                        position: node.position,
                        subnodes: 0,
                        incidents: vec![],
                    })
                    .1;
                assert!(replaced.is_none());
            }
        }
    }
    levels.push(level);
    // get the remaining levels
    loop {
        let mut level = OrdArena::<P0, PBack, HierarchyNode<PBack>>::new();
        let last_level = levels.last().unwrap();
        for (_, p_cnode, subnode) in last_level {
            if let Some(p_super) = channeler.get_supernode(*p_cnode) {
                if let Some(p0) = level.find_key(&p_super) {
                    let node = level.get_val_mut(p0).unwrap();
                    node.subnodes += 1;
                    // this will be divided by the subnode count later
                    node.position.0 += subnode.position.0;
                    node.position.1 += subnode.position.1;
                } else {
                    let _ = level.insert(p_super, HierarchyNode {
                        position: subnode.position,
                        subnodes: 1,
                        incidents: vec![],
                    });
                }
            }
        }
        if level.is_empty() {
            break
        }
        // normalized so next level uses the right positions
        for node in level.vals_mut() {
            if node.subnodes > 0 {
                node.position.0 /= i32::try_from(node.subnodes).unwrap();
                node.position.1 /= i32::try_from(node.subnodes).unwrap();
            }
        }
        levels.push(level);
    }
    // add on all edges
    for edge in channeler.cedges.vals() {
        let mut v = vec![];
        edge.incidents(|p| v.push(cnodes.get_val(p).unwrap().p_this_cnode));
        let lvl = cnodes.get_val(v[0]).unwrap().lvl;
        if let Some(level) = levels.get_mut(lvl as usize) {
            for i in 0..v.len() {
                // note we are using unidirectional edges and the incidences are not complete
                for j in (i + 1)..v.len() {
                    if let (Some(a), Some(_)) = (level.find_key(&v[i]), level.find_key(&v[j])) {
                        level.get_val_mut(a).unwrap().incidents.push(v[j]);
                    }
                }
            }
        }
    }
    // render the levels
    for (i, level) in levels.iter().enumerate() {
        let color = Render::COLORS[(i + 2) % Render::COLORS.len()];
        for node in level.vals() {
            r.circles.push((node.position, 8, color.to_owned()));
            for incident in &node.incidents {
                let p_other = level.find_key(incident).unwrap();
                let position = level.get_val(p_other).unwrap().position;
                r.lines.push((node.position, position, 4, color.to_owned()));
            }
        }
    }
}
