use std::{cmp::min, num::NonZeroUsize};

use starlight::{
    awint::{awi, awint_dag::EvalError, dag},
    triple_arena::{ptr_struct, Arena},
    Epoch, EvalAwi, LazyAwi, StarRng,
};

#[cfg(debug_assertions)]
const N: (usize, usize) = (30, 100);

#[cfg(not(debug_assertions))]
const N: (usize, usize) = (50, 1000);

ptr_struct!(P0);

#[derive(Debug, Clone)]
struct Pair {
    awi: awi::Awi,
    dag: dag::Awi,
    eval: Option<EvalAwi>,
}

#[derive(Debug)]
struct Mem {
    a: Arena<P0, Pair>,
    // `LazyAwi`s that get need to be retro assigned
    roots: Vec<(LazyAwi, awi::Awi)>,
    // the outer Vec has all supported bitwidths plus one dummy 0 bitwidth vec, the
    // inner vecs are unsorted and used for random querying
    v: Vec<Vec<P0>>,
    rng: StarRng,
}

impl Mem {
    pub fn new() -> Self {
        let mut v = vec![];
        for _ in 0..65 {
            v.push(vec![]);
        }
        Self {
            a: Arena::new(),
            roots: vec![],
            v,
            rng: StarRng::new(0),
        }
    }

    pub fn clear(&mut self) {
        self.a.clear();
        self.v.clear();
        self.roots.clear();
        for _ in 0..65 {
            self.v.push(vec![]);
        }
    }

    pub fn next(&mut self, w: usize) -> P0 {
        let try_query = self.rng.out_of_4(3);
        if try_query && (!self.v[w].is_empty()) {
            *self.rng.index_slice(&self.v[w]).unwrap()
        } else {
            let nzbw = NonZeroUsize::new(w).unwrap();
            let mut lit = awi::Awi::zero(nzbw);
            self.rng.next_bits(&mut lit);
            // Randomly make some literals and some opaques
            if self.rng.next_bool() {
                let p = self.a.insert(Pair {
                    awi: lit.clone(),
                    dag: dag::Awi::from(&lit),
                    eval: None,
                });
                self.v[w].push(p);
                p
            } else {
                let lazy = LazyAwi::opaque(nzbw);
                let p = self.a.insert(Pair {
                    awi: lit.clone(),
                    dag: dag::Awi::from(lazy.as_ref()),
                    eval: None,
                });
                self.roots.push((lazy, lit));
                self.v[w].push(p);
                p
            }
        }
    }

    /// Randomly creates a new pair or gets an existing one under the `cap`
    pub fn next_capped(&mut self, w: usize, cap: usize) -> P0 {
        if self.rng.out_of_4(3) && (!self.v[w].is_empty()) {
            let p = self.rng.index_slice(&self.v[w]).unwrap();
            if self.get(*p).awi.to_usize() < cap {
                return *p
            }
        }
        let nzbw = NonZeroUsize::new(w).unwrap();
        let lazy = LazyAwi::opaque(nzbw);
        let mut lit = awi::Awi::zero(nzbw);
        lit.usize_(self.rng.index(cap).unwrap());
        let p = self.a.insert(Pair {
            awi: lit.clone(),
            dag: dag::Awi::from(lazy.as_ref()),
            eval: None,
        });
        self.roots.push((lazy, lit));
        self.v[w].push(p);
        p
    }

    /// Calls `next` with a random integer in 1..=6, returning a tuple of the
    /// width chosen and the Ptr to what `next` returned.
    pub fn next6(&mut self) -> (usize, P0) {
        let w = ((self.rng.next_u8() as usize) % 6) + 1;
        (w, self.next(w))
    }

    pub fn next_usize(&mut self, cap: usize) -> P0 {
        self.next_capped(usize::BITS as usize, cap)
    }

    pub fn get(&self, inx: P0) -> Pair {
        self.a[inx].clone()
    }

    pub fn finish(&mut self, epoch: &Epoch) {
        for pair in self.a.vals_mut() {
            pair.eval = Some(EvalAwi::from(&pair.dag))
        }
        // then pruning can be done safely
        epoch.lower_and_prune().unwrap();
    }

    pub fn verify_equivalence(&mut self, epoch: &Epoch) -> Result<(), EvalError> {
        // set all lazy roots
        for (lazy, lit) in &mut self.roots {
            lazy.retro_(lit).unwrap();
        }

        // evaluate all
        epoch.assert_assertions(true).unwrap();
        for pair in self.a.vals() {
            assert_eq!(pair.eval.as_ref().unwrap().eval().unwrap(), pair.awi);
        }
        Ok(())
    }
}

fn operation(rng: &mut StarRng, m: &mut Mem) {
    match rng.index(4).unwrap() {
        // Copy
        0 => {
            // doesn't actually do anything on the DAG side, but we use it to get parallel
            // things in the fuzzing
            let (w, from) = m.next6();
            let to = m.next(w);
            if to != from {
                let (to, from) = m.a.get2_mut(to, from).unwrap();
                to.awi.copy_(&from.awi).unwrap();
                to.dag.copy_(&from.dag).unwrap();
            }
        }
        // Get-Set
        1 => {
            let (w0, from) = m.next6();
            let (w1, to) = m.next6();
            let usize_inx0 = rng.index(w0).unwrap();
            let usize_inx1 = rng.index(w1).unwrap();
            let b = m.a[from].awi.get(usize_inx0).unwrap();
            m.a[to].awi.set(usize_inx1, b).unwrap();
            let b = m.a[from].dag.get(usize_inx0).unwrap();
            m.a[to].dag.set(usize_inx1, b).unwrap();
        }
        // static fielding needed for interacting with the large tables
        2 => {
            let w0 = 4 << rng.index(4).unwrap();
            let w1 = 4 << rng.index(4).unwrap();
            let min_w = min(w0, w1);
            let width = m.next_usize(min_w + 1);
            let from = m.next_usize(1 + w0 - m.get(width).awi.to_usize());
            let to = m.next_usize(1 + w1 - m.get(width).awi.to_usize());
            let rhs = m.next(w0);
            let lhs = m.next(w1);

            let from_a = m.get(from);
            let to_a = m.get(to);
            let width_a = m.get(width);
            let rhs_a = m.get(rhs);
            m.a[lhs]
                .awi
                .field(
                    to_a.awi.to_usize(),
                    &rhs_a.awi,
                    from_a.awi.to_usize(),
                    width_a.awi.to_usize(),
                )
                .unwrap();
            // use the `awi` versions for the shift information
            m.a[lhs]
                .dag
                .field(
                    to_a.awi.to_usize(),
                    &rhs_a.dag,
                    from_a.awi.to_usize(),
                    width_a.awi.to_usize(),
                )
                .unwrap();
        }
        // Lut and dynamic luts
        3 => {
            let out = m.next(1);
            let (inx_w, inx) = m.next6();
            let lut = m.next(1 << inx_w);
            let lut_a = m.get(lut);
            let inx_a = m.get(inx);
            m.a[out].awi.lut_(&lut_a.awi, &inx_a.awi).unwrap();
            m.a[out].dag.lut_(&lut_a.dag, &inx_a.dag).unwrap();
        }
        _ => unreachable!(),
    }
}

#[test]
fn fuzz_elementary() {
    let mut rng = StarRng::new(0);
    let mut m = Mem::new();

    for _ in 0..N.1 {
        let epoch = Epoch::new();
        for _ in 0..N.0 {
            operation(&mut rng, &mut m)
        }
        m.finish(&epoch);
        epoch.verify_integrity().unwrap();
        let res = m.verify_equivalence(&epoch);
        res.unwrap();
        epoch.optimize().unwrap();
        let res = m.verify_equivalence(&epoch);
        res.unwrap();
        // TODO verify stable optimization
        drop(epoch);
        m.clear();
    }
}

// TODO need a version with loops and random rnodes
