use std::num::NonZeroUsize;

use starlight::{
    awint::{awi, awint_dag::EvalError, dag},
    triple_arena::{ptr_struct, Arena},
    Epoch, LazyAwi, StarRng,
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
}

#[derive(Debug)]
struct Mem {
    a: Arena<P0, Pair>,
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
        for _ in 0..65 {
            self.v.push(vec![]);
        }
    }

    pub fn next(&mut self, w: usize) -> P0 {
        let try_query = self.rng.out_of_4(3);
        if try_query && (!self.v[w].is_empty()) {
            *self.rng.index(&self.v[w]).unwrap()
        } else {
            let nzbw = NonZeroUsize::new(w).unwrap();
            let mut lit = awi::Awi::zero(nzbw);
            self.rng.next_bits(&mut lit);
            // Randomly make some literals and some opaques
            if self.rng.next_bool() {
                let p = self.a.insert(Pair {
                    awi: lit.clone(),
                    dag: dag::Awi::from(&lit),
                });
                self.v[w].push(p);
                p
            } else {
                let lazy = LazyAwi::zero(nzbw);
                let p = self.a.insert(Pair {
                    awi: lit.clone(),
                    dag: dag::Awi::from(lazy.as_ref()),
                });
                self.roots.push((lazy, lit));
                self.v[w].push(p);
                p
            }
        }
    }

    pub fn next1_5(&mut self) -> (usize, P0) {
        let w = ((self.rng.next_u8() as usize) % 4) + 1;
        (w, self.next(w))
    }

    pub fn get(&self, inx: P0) -> Pair {
        self.a[inx].clone()
    }

    pub fn verify_equivalence(&mut self, epoch: &Epoch) -> Result<(), EvalError> {
        let mut _ensemble = epoch.clone_ensemble();

        // the ensemble has a random mix of literals and opaques

        // set all lazy roots
        for (lazy, lit) in &mut self.roots {
            lazy.retro_(&lit).unwrap();
        }

        for (_, pair) in &self.a {
            let mut lazy = LazyAwi::from(pair.dag.as_ref());
            assert_eq!(lazy.eval().unwrap(), pair.awi);
        }
        Ok(())
    }
}

fn operation(rng: &mut StarRng, m: &mut Mem) {
    let next_op = rng.next_u8() % 3;
    match next_op {
        // Copy
        0 => {
            // doesn't actually do anything on the DAG side, but we use it to get parallel
            // things in the fuzzing
            let (w, from) = m.next1_5();
            let to = m.next(w);
            if to != from {
                let (to, from) = m.a.get2_mut(to, from).unwrap();
                to.awi.copy_(&from.awi).unwrap();
                to.dag.copy_(&from.dag).unwrap();
            }
        }
        // Get-Set
        1 => {
            let (w0, from) = m.next1_5();
            let (w1, to) = m.next1_5();
            let b = m.a[from].awi.get((rng.next_u32() as usize) % w0).unwrap();
            m.a[to].awi.set((rng.next_u32() as usize) % w1, b).unwrap();
            let b = m.a[from].dag.get((rng.next_u32() as usize) % w0).unwrap();
            m.a[to].dag.set((rng.next_u32() as usize) % w1, b).unwrap();
        }
        // Lut
        2 => {
            let (out_w, out) = m.next1_5();
            let (inx_w, inx) = m.next1_5();
            let lut = m.next(out_w * (1 << inx_w));
            let lut_a = m.get(lut);
            let inx_a = m.get(inx);
            m.a[out].awi.lut_(&lut_a.awi, &inx_a.awi).unwrap();
            m.a[out].dag.lut_(&lut_a.dag, &inx_a.dag).unwrap();
        }
        _ => unreachable!(),
    }
}

#[test]
fn fuzz_lower_and_eval() {
    let mut rng = StarRng::new(0);
    let mut m = Mem::new();

    for _ in 0..N.1 {
        let epoch = Epoch::new();
        for _ in 0..N.0 {
            operation(&mut rng, &mut m)
        }
        let res = m.verify_equivalence(&epoch);
        res.unwrap();
        // TODO verify stable optimization
        //let res = m.verify_equivalence(|t_dag| t_dag.optimize_basic(), &epoch);
        //res.unwrap();
        drop(epoch);
        m.clear();
    }
}

// TODO need a version with loops and random notes
