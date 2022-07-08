use awint::bw;
use rand_xoshiro::{
    rand_core::{RngCore, SeedableRng},
    Xoshiro128StarStar,
};
use starlight::Perm;

#[test]
fn perm() {
    let mut perm = Perm::ident(bw(4)).unwrap();
    perm.swap(13, 14).unwrap();
    for i in 0..16 {
        let e = perm.get(i).unwrap();
        if i == 13 {
            assert_eq!(e, 14);
        } else if i == 14 {
            assert_eq!(e, 13);
        } else {
            assert_eq!(e, i);
        }
    }
    for i in 0..16 {
        perm.unstable_set(i, 15 - i).unwrap();
    }
    assert!(perm.get(16).is_none());
    assert!(perm.unstable_set(16, 0).is_none());
}

#[test]
fn swap_and_mul() {
    let mut p0 = Perm::ident(bw(5)).unwrap();
    let mut p1 = p0.clone();
    let mut p2 = p0.clone();
    let mut tmp = p0.clone();
    let mut rng = Xoshiro128StarStar::seed_from_u64(0);
    // t_swap version
    for _ in 0..100 {
        let i0 = (rng.next_u64() as usize) % p0.l();
        let i1 = (rng.next_u64() as usize) % p0.l();
        p0.t_swap(i0, i1).unwrap();
        // when doing single swaps from identity we can use plain `swap`
        p2.swap(i0, i1).unwrap();
        tmp.mul_assign(&p1, &p2).unwrap();
        p1.copy_assign(&tmp).unwrap();
        // undo to keep `p2` as identity
        p2.swap(i0, i1).unwrap();
        assert_eq!(p0, p1);
    }
    // swap version
    for _ in 0..100 {
        let i0 = (rng.next_u64() as usize) % p0.l();
        let i1 = (rng.next_u64() as usize) % p0.l();
        p0.swap(i0, i1).unwrap();
        // when doing single swaps from identity we can use plain `swap`
        p2.swap(i0, i1).unwrap();
        tmp.mul_assign(&p2, &p1).unwrap();
        p1.copy_assign(&tmp).unwrap();
        // undo to keep `p2` as identity
        p2.swap(i0, i1).unwrap();
        assert_eq!(p0, p1);
    }
}

#[test]
fn inv_and_mul() {
    let mut p0 = Perm::ident(bw(5)).unwrap();
    let mut p1 = p0.clone();
    let mut p2 = p0.clone();
    let ident = p0.clone();
    let mut rng = Xoshiro128StarStar::seed_from_u64(0);
    // inverse on right
    for _ in 0..100 {
        p0.rand_assign_with(&mut rng);
        p1.inv_assign(&p0).unwrap();
        p2.mul_assign(&p0, &p1).unwrap();
        assert_eq!(p2, ident);
    }
    // inverse on left
    for _ in 0..100 {
        p0.rand_assign_with(&mut rng);
        p1.inv_assign(&p0).unwrap();
        p2.mul_assign(&p1, &p0).unwrap();
        assert_eq!(p2, ident);
    }
}

#[test]
fn double_and_halve() {
    let mut p0 = Perm::ident(bw(4)).unwrap();
    let mut p1 = p0.clone();
    let mut p2 = p0.clone();
    let ident = p0.clone();
    let mut rng = Xoshiro128StarStar::seed_from_u64(0);
    for _ in 0..100 {
        p0.rand_assign_with(&mut rng);
        let i = (rng.next_u32() as usize) % (p0.n() + 1);
        let p1 = p0.double(i).unwrap();
        let p2 = p1.halve(i, false).unwrap();
        let p3 = p1.halve(i, true).unwrap();
        assert_eq!(p0, p2);
        assert_eq!(p0, p3);
    }
}
