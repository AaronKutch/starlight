use starlight::misc::{Direction::*, Grid};

// copied from unit test that we do not want to format
#[test]
fn grid() {
    let grid: Grid<u64> = Grid::try_from([[0, 1, 2], [3, 4, 5]]).unwrap();

    let expected_pairs = [
        (0, 1, false),
        (1, 2, false),
        (0, 3, true),
        (3, 4, false),
        (1, 4, true),
        (4, 5, false),
        (2, 5, true),
    ];
    let mut encountered = vec![];
    grid.for_each_orthogonal_pair(|t0, _, t1, dir| encountered.push((*t0, *t1, dir)));
    assert_eq!(expected_pairs.as_slice(), encountered.as_slice());

    let grid: Grid<u64> = Grid::try_from([[0, 1, 2], [3, 4, 5], [6, 7, 8]]).unwrap();

    let expected_pairs = [
        (0, 1, 0, 0, false),
        (1, 2, 1, 0, false),
        (0, 3, 0, 0, true),
        (3, 4, 0, 1, false),
        (1, 4, 1, 0, true),
        (4, 5, 1, 1, false),
        (2, 5, 2, 0, true),
        (3, 6, 0, 1, true),
        (6, 7, 0, 2, false),
        (4, 7, 1, 1, true),
        (7, 8, 1, 2, false),
        (5, 8, 2, 1, true),
    ];
    let mut encountered = vec![];
    grid.for_each_orthogonal_pair(|t0, (i, j), t1, dir| encountered.push((*t0, *t1, i, j, dir)));
    assert_eq!(expected_pairs.as_slice(), encountered.as_slice());

    let grid: Grid<u64> = Grid::try_from([[0, 1, 2, 3], [4, 5, 6, 7], [8, 9, 10, 11]]).unwrap();

    // note 5 and 6 are skipped entirely, and the corners
    // have both edges called on separately
    let expected = [
        (0, Neg0),
        (4, Neg0),
        (8, Neg0),
        (3, Pos0),
        (7, Pos0),
        (11, Pos0),
        (0, Neg1),
        (1, Neg1),
        (2, Neg1),
        (3, Neg1),
        (8, Pos1),
        (9, Pos1),
        (10, Pos1),
        (11, Pos1),
    ];
    let mut encountered = vec![];
    grid.for_each_edge(|t, _, dir| encountered.push((*t, dir)));
    assert_eq!(expected.as_slice(), encountered.as_slice());
}
