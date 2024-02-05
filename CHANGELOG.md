# Changelog

## [0.4.0] - TODO
### Changes
- Reworked the evaluation system to be event based, now there are `drive_with_delay` functions and
  `Epoch::run*` functions with precise semantics
- Removed `LazyInlAwi`
- Removed the `Clone` impl for `EvalAwi` because it had questionable semantics and bad implications

### Additions
- `Loop`s and `Net`s can now have an arbitrary initial value

## [0.3.0] - 2024-01-22
### Fixes
- Fixed a off-by-one issue with `StarRng::out_of_*`

### Changes
- merged `Epoch::assert_assertions` and `Epoch::assert_assertions_strict`
- many fixes for `Epoch` behavior
- `LNode`s now have a `LNodeKind`
- `StarRng::index` was renamed to `index_slice`, and a `index_slice_mut` and new `index` function
  were added
- Redid the error system

### Additions
- Added `Epoch::suspend`
- Optimization now compresses allocations
- Added many primitive versions of `retro_` and `eval` functions

## [0.2.0] - 2023-12-08
### Crate
- `awint` 0.15

### Changes
- Dramatically improved performance by a variety of changes

## [0.1.0] - 2023-12-05
### Crate
- `awint` 0.14

### Additions
- Initial release with `Epoch`, `LazyAwi`, `LazyInlAwi`, and `EvalAwi`. Lowering of all mimicking
  operations to LUTs and evaluation is fully working.
