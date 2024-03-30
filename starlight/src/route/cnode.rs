use std::num::NonZeroU64;

use awint::awint_dag::triple_arena::{Recast, Recaster};

use crate::route::{Channeler, PCNode, Programmability, Sink, Source};

/// A channel node
#[derive(Debug, Clone)]
pub struct CNode {
    pub lvl: u16,
    pub p_supernode: Option<PCNode>,
    pub p_subnodes: Vec<PCNode>,
    pub sources: Vec<Source>,
    pub sinks: Vec<Sink>,
    // this counts the total number of `lvl == 0` subnodes
    pub base_subnodes: usize,
    // equivalent number of LUT bits available
    pub lut_bits: usize,
    pub programmability: Programmability,

    /// The lagrangian multiplier, fixed point such that (1 << 16) is 1.0
    pub lagrangian: u32,
    pub alg_visit: NonZeroU64,
    pub alg_usize0: usize,
    // this is used in Dijkstras' and points backwards
    pub alg_edge: (Option<PCNode>, usize),
}

impl Recast<PCNode> for CNode {
    fn recast<R: Recaster<Item = PCNode>>(
        &mut self,
        recaster: &R,
    ) -> Result<(), <R as Recaster>::Item> {
        self.p_supernode.recast(recaster)?;
        self.p_subnodes.recast(recaster)?;
        self.sources.recast(recaster)?;
        self.sinks.recast(recaster)?;
        self.alg_edge.0.recast(recaster)?;
        Ok(())
    }
}

impl CNode {
    pub fn programmability(&self) -> &Programmability {
        &self.programmability
    }

    pub fn sources(&self) -> &[Source] {
        &self.sources
    }

    pub fn sinks(&self) -> &[Sink] {
        &self.sinks
    }

    pub fn sources_mut(&mut self) -> &mut [Source] {
        &mut self.sources
    }

    pub fn sinks_mut(&mut self) -> &mut [Sink] {
        &mut self.sinks
    }

    pub fn incidents<F: FnMut(PCNode)>(&self, mut f: F) {
        for source in self.sources() {
            f(source.p_cnode)
        }
        for sink in self.sinks() {
            f(sink.p_cnode)
        }
    }
}

impl Channeler {
    /// Given the `subnodes` (which should point to unique `ThisCNode`s) for a
    /// new top level `CNode`, this will manage the backrefs. Note that
    /// `top_level_cnodes` is not set correctly by this.
    pub fn make_cnode(
        &mut self,
        mut p_subnodes: Vec<PCNode>,
        lvl: u16,
        programmability: Programmability,
    ) -> PCNode {
        p_subnodes.sort();
        let p_supernode = self.cnodes.insert(CNode {
            lvl,
            p_supernode: None,
            p_subnodes: vec![],
            sinks: vec![],
            sources: vec![],
            base_subnodes: 0,
            lut_bits: 0,
            programmability,
            lagrangian: 0,
            alg_visit: NonZeroU64::new(1).unwrap(),
            alg_usize0: 0,
            alg_edge: (None, 0),
        });
        let mut base_subnodes = if p_subnodes.is_empty() { 1usize } else { 0 };
        let mut lut_bits = 0usize;
        for p_subnode in p_subnodes.iter().copied() {
            let cnode = self.cnodes.get_mut(p_subnode).unwrap();
            base_subnodes = base_subnodes.checked_add(cnode.base_subnodes).unwrap();
            lut_bits = lut_bits.checked_add(cnode.lut_bits).unwrap();
            debug_assert!(cnode.p_supernode.is_none());
            cnode.p_supernode = Some(p_supernode);
        }
        let supernode = self.cnodes.get_mut(p_supernode).unwrap();
        supernode.base_subnodes = base_subnodes;
        supernode.lut_bits = lut_bits;
        supernode.p_subnodes = p_subnodes;
        p_supernode
    }

    #[must_use]
    pub fn get_supernode(&self, p: PCNode) -> Option<PCNode> {
        self.cnodes.get(p)?.p_supernode
    }

    /// Given two `CNode`s, this will find their lowest level common supernode
    /// (or just return the higher level of the two if one is a supernode of the
    /// other, or return one if they are equal). Can only return `None` if there
    /// are disjoint `CNode` hiearchies. If this function is used in a loop with
    /// a common accumulator, this will find the common supernode of all the
    /// nodes.
    pub fn find_common_supernode(
        &self,
        mut p_cnode0: PCNode,
        mut p_cnode1: PCNode,
    ) -> Option<PCNode> {
        let cnode0 = self.cnodes.get(p_cnode0).unwrap();
        let mut lvl0 = cnode0.lvl;
        let cnode1 = self.cnodes.get(p_cnode1).unwrap();
        let mut lvl1 = cnode1.lvl;
        // first get on same level
        loop {
            // have this run first for all cases
            if p_cnode0 == p_cnode1 {
                // case where one is the supernode of the other
                return Some(p_cnode0)
            }
            if lvl0 < lvl1 {
                p_cnode0 = self.get_supernode(p_cnode0)?;
                lvl0 += 1;
            } else if lvl0 > lvl1 {
                p_cnode1 = self.get_supernode(p_cnode1)?;
                lvl1 += 1;
            } else {
                break
            }
        }
        // find common supernode
        loop {
            p_cnode0 = self.get_supernode(p_cnode0)?;
            p_cnode1 = self.get_supernode(p_cnode1)?;
            if p_cnode0 == p_cnode1 {
                return Some(p_cnode0)
            }
        }
    }

    /// Returns  incident `PCNode`s of `p` itself and all nodes directly
    /// incident to it through edges. Node that this modifies the
    /// `alg_visit` of local nodes.
    pub fn related_nodes(&mut self, p: PCNode) -> Vec<PCNode> {
        let related_visit = self.next_alg_visit();
        let cnode = self.cnodes.get_mut(p).unwrap();
        cnode.alg_visit = related_visit;
        let mut res = vec![p];

        let others: Vec<PCNode> = cnode
            .sources
            .iter()
            .map(|source| source.p_cnode)
            .chain(cnode.sinks.iter().map(|sink| sink.p_cnode))
            .collect();
        for p_cnode in others {
            let alg_visit = &mut self.cnodes.get_mut(p_cnode).unwrap().alg_visit;
            if *alg_visit != related_visit {
                *alg_visit = related_visit;
                res.push(p_cnode);
            }
        }
        res
    }
}
