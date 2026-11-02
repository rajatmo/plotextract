//! The tick networks behind the [`TickModels`] port, run on the CPU by tract (pure Rust, nothing to install). The two
//! ONNX files are built into the program (see `models/MODEL.md`); each is made ready to run the first time it is asked
//! for, on the thread that asks, and kept.

use std::sync::OnceLock;

use plotextract_ports::{ModelError, TickModels};
use tract_onnx::prelude::*;

const FINDER: &[u8] = include_bytes!("../models/ticks.onnx");
const READER: &[u8] = include_bytes!("../models/reader.onnx");

type Plan = Arc<TypedSimplePlan>;

/// The tick finder and the reader built into the program.
#[derive(Default)]
pub struct OnnxTicks {
    finder: OnceLock<Result<Plan, String>>,
    reader: OnceLock<Result<Plan, String>>,
}

impl OnnxTicks {
    pub fn new() -> Self {
        Self::default()
    }
}

/// A network made ready for inputs of `shape`, where `n` stands for any number of crops.
fn plan(bytes: &[u8], shape: &[Option<usize>]) -> Result<Plan, String> {
    let ready = || -> TractResult<Plan> {
        let mut model = tract_onnx::onnx().model_for_read(&mut std::io::Cursor::new(bytes))?;
        let n = model.symbols.sym("n");
        let dims: TVec<TDim> = shape.iter().map(|d| d.map_or_else(|| n.clone().into(), TDim::from)).collect();
        model.set_input_fact(0, f32::fact(dims).into())?;
        model.into_optimized()?.into_runnable()
    };
    ready().map_err(|e| format!("cannot load a tick network: {e:#}"))
}

fn run(
    plan: &OnceLock<Result<Plan, String>>,
    make: impl FnOnce() -> Result<Plan, String>,
    input: Tensor,
) -> Result<Vec<f32>, ModelError> {
    let plan = plan.get_or_init(make).as_ref().map_err(|e| ModelError(e.clone()))?;
    let out = plan.run(tvec!(input.into())).map_err(|e| ModelError(format!("a tick network failed: {e:#}")))?;
    let view = out[0].to_plain_array_view::<f32>().map_err(|e| ModelError(e.to_string()))?;
    Ok(view.iter().copied().collect())
}

impl TickModels for OnnxTicks {
    fn maps(&self, square: &[f32]) -> Result<Vec<f32>, ModelError> {
        let input = Tensor::from_shape(&[1, 1, 512, 512], square).map_err(|e| ModelError(e.to_string()))?;
        run(&self.finder, || plan(FINDER, &[Some(1), Some(1), Some(512), Some(512)]), input)
    }

    fn read(&self, crops: &[f32]) -> Result<Vec<f32>, ModelError> {
        let n = crops.len() / (32 * 192);
        if n == 0 {
            return Ok(Vec::new());
        }
        let input = Tensor::from_shape(&[n, 1, 32, 192], crops).map_err(|e| ModelError(e.to_string()))?;
        run(&self.reader, || plan(READER, &[None, Some(1), Some(32), Some(192)]), input)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_networks_load_and_give_maps_and_scores_of_the_right_size() {
        let models = OnnxTicks::new();
        let maps = models.maps(&vec![1.0; 512 * 512]).unwrap();
        assert_eq!(maps.len(), 8 * 128 * 128);
        // paper only: no tick anywhere
        assert!(maps[..128 * 128].iter().all(|&m| m < 0.0));
        for n in [1, 3] {
            let scores = models.read(&vec![1.0; n * 32 * 192]).unwrap();
            assert_eq!(scores.len(), n * 48 * 14);
        }
        assert!(models.read(&[]).unwrap().is_empty());
    }
}
