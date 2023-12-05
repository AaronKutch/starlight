use std::path::PathBuf;

use starlight::{awint_dag::EvalError, Epoch};

fn _render(epoch: &Epoch) -> Result<(), EvalError> {
    epoch.render_to_svgs_in_dir(PathBuf::from("./".to_owned()))
}
